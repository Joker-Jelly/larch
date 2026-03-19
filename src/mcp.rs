use rmcp::{ServerHandler, model::{ServerInfo, ServerCapabilities}, schemars, tool, tool_handler, tool_router, ServiceExt};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use std::sync::{Arc, Mutex};

use crate::config::VaultConfig;
use crate::index::{self, SchemaFields};

// ── Shared state ───────────────────────────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub index: Arc<tantivy::Index>,
    pub fields: SchemaFields,
    pub config: VaultConfig,
    pub reader: Arc<tantivy::IndexReader>,
    pub writer: Arc<Mutex<tantivy::IndexWriter>>,
    tool_router: ToolRouter<Self>,
}

// ── Tools ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The search query
    pub query: String,
    /// Maximum number of results to return (default: 10)
    pub limit: Option<usize>,
    /// Optional tag to filter by
    pub tag: Option<String>,
    /// Optional directory to filter by
    pub dir: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReadContextArgs {
    /// Absolute or vault-relative path to the markdown file
    pub path: String,
    /// Start line number (1-indexed)
    pub start_line: u64,
    /// End line number (inclusive)
    pub end_line: u64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct TreeArgs {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct TagArgs {
    /// Optional tag to filter by. If omitted, returns all tags.
    pub tag: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ImportArgs {
    /// The markdown content to import
    pub content: String,
    /// Filename for the imported document (e.g. "my-note.md")
    pub filename: String,
    /// Optional sub-directory within the vault to place the file
    pub dir: Option<String>,
}

#[tool_router]
impl McpState {
    #[tool(
        name = "tags",
        description = "Get all tags or documents associated with a specific tag."
    )]
    async fn tags(
        &self,
        Parameters(args): Parameters<TagArgs>,
    ) -> Result<String, String> {
        if let Some(t) = args.tag {
            let paths = crate::tag::get_files_for_tag(&self.index, self.fields.tags, &t)
                .map_err(|e| e.to_string())?;
            let json = serde_json::to_string_pretty(&paths)
                .map_err(|e| e.to_string())?;
            Ok(json)
        } else {
            let tags = crate::tag::get_all_tags(&self.index, self.fields.tags)
                .map_err(|e| e.to_string())?;
            let json = serde_json::to_string_pretty(&tags)
                .map_err(|e| e.to_string())?;
            Ok(json)
        }
    }

    #[tool(
        name = "tree",
        description = "Get the complete directory tree of the vault."
    )]
    async fn tree(
        &self,
        Parameters(_args): Parameters<TreeArgs>,
    ) -> Result<String, String> {
        let root = crate::tree::build_tree(&self.config.vault_root);
        let json = serde_json::to_string_pretty(&root)
            .map_err(|e| e.to_string())?;
        Ok(json)
    }

    #[tool(
        name = "search",
        description = "Search the local Larch Markdown knowledge base using keywords."
    )]
    async fn search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<String, String> {
        let limit = args.limit.unwrap_or(10);
        let searcher = self.reader.searcher();
        let results = index::search(
            &searcher,
            &self.fields,
            &args.query,
            args.tag.as_deref(),
            args.dir.as_deref(),
            limit,
            true, // plain snippets for MCP
        )
            .map_err(|e| e.to_string())?;

        let json = serde_json::to_string_pretty(&results)
            .map_err(|e| e.to_string())?;
        Ok(json)
    }

    #[tool(
        name = "document",
        description = "Read precise line ranges from a local Markdown file to gain deeper context."
    )]
    async fn document(
        &self,
        Parameters(args): Parameters<ReadContextArgs>,
    ) -> Result<String, String> {
        let lines = crate::document::read_document_range(
            &self.config.vault_root,
            &args.path,
            Some(args.start_line as usize),
            Some(args.end_line as usize),
        ).map_err(|e| e.to_string())?;

        Ok(lines)
    }

    #[tool(
        name = "import",
        description = "Import markdown content into the vault. Creates a new file, processes assets, and indexes it for search."
    )]
    async fn import(
        &self,
        Parameters(args): Parameters<ImportArgs>,
    ) -> Result<String, String> {
        let target_dir = if let Some(ref dir) = args.dir {
            self.config.vault_root.join(dir)
        } else {
            self.config.vault_root.clone()
        };

        std::fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Creating directory: {}", e))?;

        let file_path = target_dir.join(&args.filename);

        // Prevent path traversal outside vault
        let canonical_vault = std::fs::canonicalize(&self.config.vault_root)
            .map_err(|e| format!("Resolving vault root: {}", e))?;
        let canonical_file = if file_path.exists() {
            std::fs::canonicalize(&file_path).map_err(|e| format!("Resolving file path: {}", e))?
        } else {
            // For new files, canonicalize the parent then append filename
            let parent = file_path.parent().unwrap_or(&target_dir);
            std::fs::canonicalize(parent)
                .map_err(|e| format!("Resolving parent dir: {}", e))?
                .join(file_path.file_name().unwrap_or_default())
        };
        if !canonical_file.starts_with(&canonical_vault) {
            return Err("Path escapes vault boundary".to_string());
        }

        // Process assets in the content (rewrite paths)
        let processed = crate::assets::process_assets(
            &args.content,
            &target_dir,
            &self.config.vault_root,
            &target_dir,
        )
        .map_err(|e| format!("Processing assets: {}", e))?;

        std::fs::write(&file_path, &processed)
            .map_err(|e| format!("Writing file: {}", e))?;

        // Parse and index using relative paths
        let file_path_str = file_path
            .strip_prefix(&self.config.vault_root)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .to_string();
        let result = crate::parser::parse_content(&processed, &file_path_str)
            .map_err(|e| format!("Parsing: {}", e))?;

        let chunks_count = result.chunks.len();
        let mut writer = self.writer.lock().unwrap();
        index::index_file(&writer, &self.fields, &file_path_str, &result.chunks, &result.meta)
            .map_err(|e| format!("Indexing: {}", e))?;
        writer.commit()
            .map_err(|e| format!("Commit: {}", e))?;

        let response = serde_json::json!({
            "success": true,
            "path": file_path_str,
            "chunks_indexed": chunks_count,
        });
        serde_json::to_string_pretty(&response)
            .map_err(|e| e.to_string())
    }
}

#[tool_handler]
impl ServerHandler for McpState {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Larch MCP Server")
    }
}

// ── Server startup ─────────────────────────────────────────────────

pub async fn run_stdio_server(config: VaultConfig, index: tantivy::Index, fields: index::SchemaFields) -> anyhow::Result<()> {
    let reader = index::create_reader(&index)?;
    let writer = index::create_writer(&index)?;

    let state = McpState {
        index: Arc::new(index),
        fields,
        config,
        reader: Arc::new(reader),
        writer: Arc::new(Mutex::new(writer)),
        tool_router: McpState::tool_router(),
    };

    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let server = state.serve(transport).await?;
    let _ = server.waiting().await;

    Ok(())
}
