use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::config::VaultConfig;
use crate::index::{self, SchemaFields, SearchResult};

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

// ── Router ─────────────────────────────────────────────────────────

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/search", get(search_handler))
        .route("/api/v1/document", get(document_handler))
        .route("/api/v1/import", post(import_handler))
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
