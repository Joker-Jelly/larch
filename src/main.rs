use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use walkdir::WalkDir;

use larch::config::{self as vault_config, VaultConfig};
use larch::{client, import, index, lockfile, mcp, parser, server, watcher};

use larch::utils::is_markdown;

#[derive(Parser)]
#[command(name = "larch")]
#[command(about = "Local-first Markdown AI Knowledge Engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// ... helper to init config, index, fields ...
fn init_context() -> Result<(VaultConfig, tantivy::Index, index::SchemaFields)> {
    let vault_dir = vault_config::get_vault_dir()?;
    let config = VaultConfig::open(&vault_dir)?;
    let (tantivy_index, fields) = index::open_or_create(&config.index_dir())?;
    Ok((config, tantivy_index, fields))
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize vault (create .larch/, assets/, config). NO indexing.
    Init {
        /// Optional path for the vault directory (default: ~/.larch-vault)
        path: Option<PathBuf>,
    },
    /// Start watcher + REST API
    Serve {
        /// Port to run the REST API on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
        /// Also start MCP server over stdin/stdout
        #[arg(long)]
        mcp: bool,
    },
    /// Search and print results
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
        /// Filter by directory
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Retrieve a specific document, optionally specifying start and end lines
    Document {
        /// File path to retrieve
        path: String,
        /// Start line (1-indexed)
        #[arg(short, long)]
        start_line: Option<usize>,
        /// End line (inclusive)
        #[arg(short, long)]
        end_line: Option<usize>,
    },
    /// Print the vault directory tree
    Tree {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Tag operations
    Tag {
        #[command(subcommand)]
        command: TagCommands,
    },
    /// Import file OR directory into vault
    Import {
        /// File or directory to import
        path: PathBuf,
        /// Move instead of copy
        #[arg(short = 'x', long)]
        move_file: bool,
        /// Sub-directory within vault to place the imported file(s)
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Structured vault state
    Status,
    /// Print recent logs
    Logs {
        /// Tail logs in real-time
        #[arg(short, long)]
        follow: bool,
    },
    /// Start MCP server (STDIO)
    Mcp,
    /// Rebuild the entire search index from the current vault files
    Reindex,
}

#[derive(Subcommand)]
enum TagCommands {
    /// List all tags or view documents for a specific tag
    Ls {
        /// Optional tag to filter by
        tag: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup basic tracing for console out / stderr
    // CRITICAL: We MUST write to stderr, because stdout is used for the MCP stdio transport.
    // If logs leak into stdout, they corrupt the JSON-RPC messages.
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let cli = Cli::parse();

    match &cli.command {
        Commands::Init { path } => {
            let dir = match path {
                Some(p) => std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()),
                None => vault_config::default_vault_dir()?,
            };
            let config = VaultConfig::init(&dir)?;

            // Save to global config
            vault_config::save_global_config(&vault_config::GlobalConfig {
                vault_path: config.vault_root.clone(),
            })?;

            println!("✅ Initialized Larch vault at {}", config.vault_root.display());
            println!("   You can now use `larch import <path>` to add markdown files.");
        }
        Commands::Serve { port, mcp: mcp_mode } => {
            let (config, tantivy_index, fields) = init_context()?;
            let reader = index::create_reader(&tantivy_index)?;
            let writer = index::create_writer(&tantivy_index)?;
            let writer_arc = Arc::new(Mutex::new(writer));

            // Write lock file so CLI commands can detect running serve
            let lock_path = config.serve_lock_path();
            lockfile::write_lock(&lock_path, *port)?;

            let rx = watcher::start_watcher(&config)?;

            // Spawn watcher event loop
            let fields_clone = fields.clone();
            let writer_clone = Arc::clone(&writer_arc);
            let config_clone = config.clone();
            tokio::spawn(async move {
                watcher::process_events(rx, fields_clone, writer_clone, config_clone).await;
            });

            // Optionally spawn MCP stdio server (shares writer/reader)
            if *mcp_mode {
                let mcp_state = mcp::create_state(
                    Arc::new(tantivy_index.clone()),
                    fields.clone(),
                    config.clone(),
                    Arc::new(reader.clone()),
                    Arc::clone(&writer_arc),
                );
                tokio::spawn(async move {
                    if let Err(e) = mcp::run_stdio_transport(mcp_state).await {
                        eprintln!("MCP server error: {}", e);
                    }
                });
            }

            // Start API server
            let state = Arc::new(server::AppState {
                index: tantivy_index,
                fields,
                config: config.clone(),
                writer: writer_arc,
                reader,
            });
            let app = server::build_router(state);
            let addr = format!("0.0.0.0:{}", port);
            let listener = TcpListener::bind(&addr).await?;
            eprintln!("🚀 Larch server running on http://{}", addr);

            // Graceful shutdown: wait for either server exit or ctrl-c
            tokio::select! {
                result = axum::serve(listener, app) => {
                    result?;
                }
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\nShutting down...");
                }
            }

            // Clean up lock file
            lockfile::remove_lock(&lock_path);
        }
        Commands::Search { query, limit, tag, dir } => {
            let (_config, tantivy_index, fields) = init_context()?;
            let reader = index::create_reader(&tantivy_index)?;
            let searcher = reader.searcher();

            let results = index::search(
                &searcher,
                &fields,
                query,
                tag.as_deref(),
                dir.as_deref(),
                *limit,
                false, // ANSI snippets for CLI
            )?;
            if results.is_empty() {
                println!("No results found for '{}'", query);
            } else {
                for (i, res) in results.iter().enumerate() {
                    println!("\n[{}] \x1b[1;36m{}\x1b[0m (Score: {:.2})", i + 1, res.file_path, res.score);
                    if !res.title_hierarchy.is_empty() {
                        println!("    \x1b[33m{}\x1b[0m", res.title_hierarchy);
                    }
                    if !res.tags.is_empty() {
                        println!("    Tags: {}", res.tags.join(", "));
                    }
                    println!("    Lines: {}-{}", res.start_line, res.end_line);
                    println!("    {}", res.content_snippet.replace('\n', " "));
                }
            }
        }
        Commands::Document { path, start_line, end_line } => {
            let (config, _tantivy_index, _fields) = init_context()?;
            let content = larch::document::read_document_range(
                &config.vault_root,
                path,
                *start_line,
                *end_line,
            )?;
            println!("{}", content);
        }
        Commands::Tree { json } => {
            let (config, _tantivy_index, _fields) = init_context()?;
            let root_node = larch::tree::build_tree(&config.vault_root);
            if *json {
                println!("{}", serde_json::to_string_pretty(&root_node)?);
            } else {
                larch::tree::print_tree(&root_node, "", true, true);
            }
        }
        Commands::Tag { command } => match command {
            TagCommands::Ls { tag, json } => {
                let (_config, tantivy_index, fields) = init_context()?;
                
                if let Some(t) = tag {
                    let paths = larch::tag::get_files_for_tag(&tantivy_index, &fields, t)?;
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&paths)?);
                    } else {
                        println!("Tag: {}", t);
                        if paths.is_empty() {
                            println!("  (No documents found)");
                        } else {
                            for path in paths {
                                println!("  - {}", path);
                            }
                        }
                    }
                } else {
                    let mut tags: Vec<_> = larch::tag::get_all_tags(&tantivy_index, &fields)?.into_iter().collect();
                    tags.sort_by(|a, b| a.0.cmp(&b.0));
                    
                    if *json {
                        let map: std::collections::HashMap<_, _> = tags.into_iter().collect();
                        println!("{}", serde_json::to_string_pretty(&map)?);
                    } else if tags.is_empty() {
                        println!("No tags found.");
                    } else {
                        for (t, paths) in tags {
                            println!("├── {}", t);
                            for (i, path) in paths.iter().enumerate() {
                                let connector = if i == paths.len() - 1 { "└──" } else { "├──" };
                                println!("│   {} {}", connector, path);
                            }
                        }
                    }
                }
            }
        },
        Commands::Import { path, move_file, dir } => {
            let (config, tantivy_index, fields) = init_context()?;
            let source_path = std::fs::canonicalize(path)?;

            // If serve is running, delegate via HTTP
            if let Some(info) = lockfile::read_lock(&config.serve_lock_path()) {
                let c = client::ServeClient::new(info.port);
                if c.is_healthy().await {
                    let result = c.import_file(
                        &source_path.to_string_lossy(),
                        *move_file,
                        dir.as_deref(),
                    ).await?;
                    println!("✅ Imported {} file(s) via serve (port {})", result.files_imported, info.port);
                    return Ok(());
                }
                // Serve not reachable — stale lock, fall through to direct write
                eprintln!("⚠️  Stale lock file detected, writing directly.");
                lockfile::remove_lock(&config.serve_lock_path());
            }

            // Direct write (no serve running)
            let writer = index::create_writer(&tantivy_index)?;
            let writer_arc = Arc::new(Mutex::new(writer));

            let target_sub_dir = dir.as_deref().unwrap_or("");
            let target_md_base = config.vault_root.join(target_sub_dir);
            std::fs::create_dir_all(&target_md_base)?;

            if source_path.is_dir() {
                let mut imported = 0;
                for entry in WalkDir::new(&source_path).into_iter().filter_map(|e| e.ok()) {
                    let p = entry.path();
                    if p.is_file() && is_markdown(p) {
                        import::import_file_from_disk(p, &source_path, &target_md_base, &config, &fields, &writer_arc, *move_file)?;
                        imported += 1;
                    }
                }
                writer_arc.lock().unwrap_or_else(|e| e.into_inner()).commit()?;
                println!("✅ Imported {} markdown files.", imported);
            } else if source_path.is_file() && is_markdown(&source_path) {
                let source_dir = source_path.parent().unwrap_or(Path::new(""));
                import::import_file_from_disk(&source_path, source_dir, &target_md_base, &config, &fields, &writer_arc, *move_file)?;
                writer_arc.lock().unwrap_or_else(|e| e.into_inner()).commit()?;
                println!("✅ Imported {}", source_path.file_name().unwrap_or_default().to_string_lossy());
            } else {
                anyhow::bail!("Path is neither a directory nor a markdown file.");
            }
        }
        Commands::Status => {
            let (config, tantivy_index, _) = init_context()?;
            
            let mut md_count = 0;
            for entry in WalkDir::new(&config.vault_root).into_iter().filter_map(|e| e.ok()) {
                if !entry.path().starts_with(config.larch_dir()) && is_markdown(entry.path()) {
                    md_count += 1;
                }
            }

            let chunk_count = index::doc_count(&tantivy_index).unwrap_or(0);
            
            println!("===============================");
            println!(" Larch Vault Status");
            println!("===============================");
            println!(" Vault Root:  {}", config.vault_root.display());
            println!(" MD Files:    {}", md_count);
            println!(" Indexed:     {} chunks", chunk_count);
            println!(" Assets Dir:  {}", config.assets_dir().display());
            println!("===============================");
        }
        Commands::Logs { follow } => {
            // Simplified log read mechanism (would realistically read from logs_dir files)
            let (config, _tantivy_index, _fields) = init_context()?;
            let logs_dir = config.logs_dir();
            println!("Logs directory: {}", logs_dir.display());
            if *follow {
                println!("(Following logs not fully implemented in MVP. Run server to see stdout logs)");
            } else {
                println!("(Log reading not fully implemented. Run server to see stdout logs)");
            }
        }
        Commands::Mcp => {
            let (config, tantivy_index, fields) = init_context()?;

            // Warn if serve is already running (concurrent writers are unsafe)
            if let Some(info) = lockfile::read_lock(&config.serve_lock_path()) {
                eprintln!(
                    "⚠️  larch serve is running (PID {}, port {}). \
                     Running MCP separately risks write conflicts. \
                     Consider `larch serve --mcp` instead.",
                    info.pid, info.port
                );
            }

            mcp::run_stdio_server(config, tantivy_index, fields).await?;
        }
        Commands::Reindex => {
            let (config, _tantivy_index, _fields) = init_context()?;

            // If serve is running, delegate via HTTP
            if let Some(info) = lockfile::read_lock(&config.serve_lock_path()) {
                let c = client::ServeClient::new(info.port);
                if c.is_healthy().await {
                    let result = c.reindex().await?;
                    println!("✅ Reindexed {} files via serve (port {})", result.files_indexed, info.port);
                    return Ok(());
                }
                eprintln!("⚠️  Stale lock file detected, reindexing directly.");
                lockfile::remove_lock(&config.serve_lock_path());
            }

            // Direct reindex (no serve running)
            let index_dir = config.index_dir();
            println!("Clearing old index at {}...", index_dir.display());
            if index_dir.exists() {
                std::fs::remove_dir_all(&index_dir)?;
            }
            std::fs::create_dir_all(&index_dir)?;

            let (tantivy_index, fields) = index::open_or_create(&index_dir)?;
            let writer = index::create_writer(&tantivy_index)?;
            let writer_arc = Arc::new(Mutex::new(writer));

            let mut count = 0;
            for entry in WalkDir::new(&config.vault_root).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if !path.starts_with(config.larch_dir()) && is_markdown(path) {
                    println!("Indexing: {}", path.display());
                    let source = std::fs::read_to_string(path)?;
                    let rel_path_str = path
                        .strip_prefix(&config.vault_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();

                    if let Ok(result) = parser::parse_content(&source, &rel_path_str) {
                         let w = writer_arc.lock().unwrap_or_else(|e| e.into_inner());
                         if let Err(e) = index::index_file(&w, &fields, &rel_path_str, &result.chunks, &result.meta) {
                             eprintln!("Index error for {}: {}", rel_path_str, e);
                         }
                         count += 1;
                    }
                }
            }
            writer_arc.lock().unwrap_or_else(|e| e.into_inner()).commit()?;
            println!("✅ Successfully re-indexed {} markdown files.", count);
        }
    }

    Ok(())
}
