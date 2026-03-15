use rmcp::{ServerHandler, model::{ServerInfo, ServerCapabilities}, schemars, tool, tool_handler, tool_router, ServiceExt};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use std::sync::Arc;

use crate::config::VaultConfig;
use crate::index::{self, SchemaFields};

// ── Shared state ───────────────────────────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub index: Arc<tantivy::Index>,
    pub fields: SchemaFields,
    pub config: VaultConfig,
    tool_router: ToolRouter<Self>,
}

// ── Tools ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The search query
    pub query: String,
    /// Maximum number of results to return (default: 10)
    pub limit: Option<usize>,
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

#[tool_router]
impl McpState {
    #[tool(
        name = "search",
        description = "Search the local Larch Markdown knowledge base using keywords."
    )]
    async fn search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<String, String> {
        let limit = args.limit.unwrap_or(10);
        let results = index::search(&self.index, &self.fields, &args.query, limit)
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
}

#[tool_handler]
impl ServerHandler for McpState {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Larch MCP Server")
    }
}

// ── Server startup ─────────────────────────────────────────────────

pub async fn run_stdio_server(config: VaultConfig) -> anyhow::Result<()> {
    let index_dir = config.index_dir();
    let (index, fields) = index::open_or_create(&index_dir)?;

    let state = McpState {
        index: Arc::new(index),
        fields,
        config,
        tool_router: McpState::tool_router(),
    };

    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let server = state.serve(transport).await?;
    let _ = server.waiting().await;

    Ok(())
}
