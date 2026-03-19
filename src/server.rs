use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

use crate::config::VaultConfig;
use crate::index::{self, SchemaFields, SearchResult};
use crate::utils::is_markdown;

// ── Shared state ───────────────────────────────────────────────────

pub struct AppState {
    pub index: tantivy::Index,
    pub fields: SchemaFields,
    pub config: VaultConfig,
    pub writer: Arc<Mutex<tantivy::IndexWriter>>,
    pub reader: tantivy::IndexReader,
}

// ── Query / body types ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub limit: Option<usize>,
    pub tag: Option<String>,
    pub dir: Option<String>,
}

#[derive(Deserialize)]
pub struct TagQuery {
    pub tag: Option<String>,
}

#[derive(Deserialize)]
pub struct DocumentQuery {
    pub path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Deserialize)]
pub struct ImportBody {
    pub content: String,
    pub filename: String,
    pub dir: Option<String>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub vault_root: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct ImportResponse {
    pub success: bool,
    pub path: String,
    pub chunks_indexed: usize,
}

#[derive(Serialize)]
pub struct DocumentResponse {
    pub path: String,
    pub start_line: usize,
    pub end_line: Option<usize>,
    pub content: String,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Deserialize)]
struct ImportFileBody {
    source_path: String,
    #[serde(default)]
    move_file: bool,
    dir: Option<String>,
}

#[derive(Serialize)]
struct ImportFileResponse {
    success: bool,
    files_imported: usize,
}

#[derive(Serialize)]
struct ReindexResponse {
    success: bool,
    files_indexed: usize,
}

// ── Router ─────────────────────────────────────────────────────────

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/search", get(search_handler))
        .route("/api/v1/document", get(document_handler))
        .route("/api/v1/import", post(import_handler))
        .route("/api/v1/import/file", post(import_file_handler))
        .route("/api/v1/reindex", post(reindex_handler))
        .route("/api/v1/tree", get(tree_handler))
        .route("/api/v1/tags", get(tag_handler))
        .with_state(state)
}

// ── Handlers ───────────────────────────────────────────────────────

async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        vault_root: state.config.vault_root.display().to_string(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn search_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(10).min(1000);
    let searcher = state.reader.searcher();
    index::search(
        &searcher,
        &state.fields,
        &params.query,
        params.tag.as_deref(),
        params.dir.as_deref(),
        limit,
        true, // plain snippets for JSON API
    )
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

async fn document_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DocumentQuery>,
) -> Result<Json<DocumentResponse>, (StatusCode, Json<ErrorResponse>)> {
    let content = crate::document::read_document_range(
        &state.config.vault_root,
        &params.path,
        params.start_line,
        params.end_line,
    ).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    let start = params.start_line.unwrap_or(1).max(1);

    Ok(Json(DocumentResponse {
        path: params.path,
        start_line: start,
        end_line: params.end_line,
        content,
    }))
}

async fn import_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ImportBody>,
) -> Result<Json<ImportResponse>, (StatusCode, Json<ErrorResponse>)> {
    let config = state.config.clone();
    let fields = state.fields.clone();
    let writer = Arc::clone(&state.writer);

    let result = tokio::task::spawn_blocking(move || {
        crate::import::import_content(
            &body.content,
            &body.filename,
            body.dir.as_deref(),
            &config,
            &fields,
            &writer,
        )
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Task join error: {}", e),
            }),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    Ok(Json(ImportResponse {
        success: true,
        path: result.rel_path,
        chunks_indexed: result.chunks_indexed,
    }))
}

async fn import_file_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ImportFileBody>,
) -> Result<Json<ImportFileResponse>, (StatusCode, Json<ErrorResponse>)> {
    let config = state.config.clone();
    let fields = state.fields.clone();
    let writer = Arc::clone(&state.writer);

    let result = tokio::task::spawn_blocking(move || {
        let source_path = std::path::PathBuf::from(&body.source_path);
        if !source_path.exists() {
            anyhow::bail!("Source path does not exist: {}", source_path.display());
        }

        let target_md_base = if let Some(ref d) = body.dir {
            config.vault_root.join(d)
        } else {
            config.vault_root.clone()
        };
        std::fs::create_dir_all(&target_md_base)
            .map_err(|e| anyhow::anyhow!("Creating target directory: {}", e))?;

        let mut files_imported = 0usize;

        if source_path.is_dir() {
            for entry in WalkDir::new(&source_path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() && is_markdown(path) {
                    crate::import::import_file_from_disk(
                        path,
                        &source_path,
                        &target_md_base,
                        &config,
                        &fields,
                        &writer,
                        body.move_file,
                    )?;
                    files_imported += 1;
                }
            }
        } else {
            crate::import::import_file_from_disk(
                &source_path,
                source_path.parent().unwrap_or(&source_path),
                &target_md_base,
                &config,
                &fields,
                &writer,
                body.move_file,
            )?;
            files_imported = 1;
        }

        let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
        w.commit().map_err(|e| anyhow::anyhow!("Committing index: {}", e))?;

        Ok::<usize, anyhow::Error>(files_imported)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Task join error: {}", e),
            }),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    Ok(Json(ImportFileResponse {
        success: true,
        files_imported: result,
    }))
}

async fn reindex_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ReindexResponse>, (StatusCode, Json<ErrorResponse>)> {
    let config = state.config.clone();
    let fields = state.fields.clone();
    let writer = Arc::clone(&state.writer);

    let result = tokio::task::spawn_blocking(move || {
        let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());

        index::delete_all_documents(&w)
            .map_err(|e| anyhow::anyhow!("Deleting all documents: {}", e))?;

        let larch_dir = config.larch_dir();
        let mut files_indexed = 0usize;

        for entry in WalkDir::new(&config.vault_root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.starts_with(&larch_dir) {
                continue;
            }
            if path.is_file() && is_markdown(path) {
                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let rel_path = path
                    .strip_prefix(&config.vault_root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                let parsed = match crate::parser::parse_content(&content, &rel_path) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if index::index_file(&w, &fields, &rel_path, &parsed.chunks, &parsed.meta).is_ok() {
                    files_indexed += 1;
                }
            }
        }

        w.commit().map_err(|e| anyhow::anyhow!("Committing index: {}", e))?;

        Ok::<usize, anyhow::Error>(files_indexed)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Task join error: {}", e),
            }),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    Ok(Json(ReindexResponse {
        success: true,
        files_indexed: result,
    }))
}

async fn tree_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::tree::TreeNode>, (StatusCode, Json<ErrorResponse>)> {
    let root = crate::tree::build_tree(&state.config.vault_root);
    Ok(Json(root))
}

async fn tag_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TagQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if let Some(t) = params.tag {
        let paths = crate::tag::get_files_for_tag(&state.index, &state.fields, &t).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() }))
        })?;
        Ok(Json(serde_json::json!(paths)))
    } else {
        let tags = crate::tag::get_all_tags(&state.index, &state.fields).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() }))
        })?;
        Ok(Json(serde_json::json!(tags)))
    }
}
