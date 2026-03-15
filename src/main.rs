use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use walkdir::WalkDir;

use larch::config::VaultConfig;
use larch::{assets, index, mcp, parser, server, watcher};

#[derive(Parser)]
#[command(name = "larch")]
#[command(about = "Local-first Markdown AI Knowledge Engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize vault (create .larch/, assets/, config). NO indexing.
    Init,
    /// Start watcher + REST API
    Serve {
        /// Port to run the REST API on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
    /// Search and print results
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
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
        Commands::Init => {
            let dir = get_default_vault_dir()?;
            let config = VaultConfig::init(&dir)?;
            println!("✅ Initialized Larch vault at {}", config.vault_root.display());
            println!("   You can now use `larch import <path>` to add markdown files.");
        }
        Commands::Serve { port } => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            let (tantivy_index, fields) = index::open_or_create(&config.index_dir())?;
            let writer = index::create_writer(&tantivy_index)?;
            let writer_arc = Arc::new(Mutex::new(writer));

            let rx = watcher::start_watcher(&config)?;
            
            // Spawn watcher event loop
            let index_clone = tantivy_index.clone();
            let fields_clone = fields.clone();
            let writer_clone = Arc::clone(&writer_arc);
            let config_clone = config.clone();
            tokio::spawn(async move {
                watcher::process_events(rx, index_clone, fields_clone, writer_clone, config_clone).await;
            });

            // Start API server
            let state = Arc::new(server::AppState {
                index: tantivy_index,
                fields,
                config: config.clone(),
                writer: writer_arc,
            });
            let app = server::build_router(state);
            let addr = format!("0.0.0.0:{}", port);
            let listener = TcpListener::bind(&addr).await?;
            println!("🚀 Larch server running on http://{}", addr);
            axum::serve(listener, app).await?;
        }
        Commands::Search { query, limit } => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            let (tantivy_index, fields) = index::open_or_create(&config.index_dir())?;
            
            let results = index::search(&tantivy_index, &fields, query, *limit)?;
            if results.is_empty() {
                println!("No results found for '{}'", query);
            } else {
                for (i, res) in results.iter().enumerate() {
                    println!("\n[{}] \x1b[1;36m{}\x1b[0m (Score: {:.2})", i + 1, res.file_path, res.score);
                    if !res.title_hierarchy.is_empty() {
                        println!("    \x1b[33m{}\x1b[0m", res.title_hierarchy);
                    }
                    println!("    Lines: {}-{}", res.start_line, res.end_line);
                    println!("    {}", res.content_snippet.replace('\n', " "));
                }
            }
        }
        Commands::Document { path, start_line, end_line } => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            let content = larch::document::read_document_range(
                &config.vault_root,
                path,
                *start_line,
                *end_line,
            )?;
            println!("{}", content);
        }
        Commands::Import { path, move_file, dir } => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            let (tantivy_index, fields) = index::open_or_create(&config.index_dir())?;
            let writer = index::create_writer(&tantivy_index)?;
            let writer_arc = Arc::new(Mutex::new(writer));

            let target_sub_dir = dir.as_deref().unwrap_or("");
            let target_md_base = config.vault_root.join(target_sub_dir);
            std::fs::create_dir_all(&target_md_base)?;

            let source_path = std::fs::canonicalize(path)?;
            if source_path.is_dir() {
                let mut imported = 0;
                for entry in WalkDir::new(&source_path).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() && is_markdown(path) {
                        import_single_file(path, &source_path, &target_md_base, &config, &fields, &writer_arc, *move_file)?;
                        imported += 1;
                    }
                }
                println!("✅ Imported {} markdown files.", imported);
            } else if source_path.is_file() && is_markdown(&source_path) {
                // If it's a single file, the base relative dir is just its parent
                let source_dir = source_path.parent().unwrap_or(Path::new(""));
                import_single_file(&source_path, source_dir, &target_md_base, &config, &fields, &writer_arc, *move_file)?;
                println!("✅ Imported {}", source_path.file_name().unwrap_or_default().to_string_lossy());
            } else {
                anyhow::bail!("Path is neither a directory nor a markdown file.");
            }
            
            writer_arc.lock().unwrap().commit()?;
        }
        Commands::Status => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            let (tantivy_index, _) = index::open_or_create(&config.index_dir())?;
            
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
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            let logs_dir = config.logs_dir();
            println!("Logs directory: {}", logs_dir.display());
            if *follow {
                println!("(Following logs not fully implemented in MVP. Run server to see stdout logs)");
            } else {
                println!("(Log reading not fully implemented. Run server to see stdout logs)");
            }
        }
        Commands::Mcp => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
            mcp::run_stdio_server(config).await?;
        }
        Commands::Reindex => {
            let config = VaultConfig::open(&get_default_vault_dir()?)?;
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
                         let w = writer_arc.lock().unwrap();
                         if let Err(e) = index::index_file(&w, &fields, &rel_path_str, &result.chunks, &result.meta) {
                             eprintln!("Index error for {}: {}", rel_path_str, e);
                         }
                         count += 1;
                    }
                }
            }
            writer_arc.lock().unwrap().commit()?;
            println!("✅ Successfully re-indexed {} markdown files.", count);
        }
    }

    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext == "md" || ext == "markdown")
        .unwrap_or(false)
}

fn get_default_vault_dir() -> Result<PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
    Ok(home_dir.join(".larch"))
}

fn import_single_file(
    file_path: &Path,
    source_base_dir: &Path,
    target_md_base: &Path,
    config: &VaultConfig,
    fields: &index::SchemaFields,
    writer_arc: &Arc<Mutex<tantivy::IndexWriter>>,
    move_file: bool,
) -> Result<()> {
    // Determine relative path structure
    let rel_path = file_path.strip_prefix(source_base_dir).unwrap_or(Path::new(file_path.file_name().unwrap()));
    let target_file_path = target_md_base.join(rel_path);
    
    if let Some(parent) = target_file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Reading {}", file_path.display()))?;

    // Process assets (copy/hash/rewrite)
    let processed_content = assets::process_assets(
        &content,
        file_path.parent().unwrap_or(Path::new("")),
        &config.vault_root,
        target_file_path.parent().unwrap_or(Path::new("")),
    )?;

    std::fs::write(&target_file_path, &processed_content)
        .with_context(|| format!("Writing to {}", target_file_path.display()))?;

    if move_file {
        let _ = std::fs::remove_file(file_path);
    }

    // Parse and Index (using relative paths for clean CLI output)
    let file_path_str = target_file_path
        .strip_prefix(&config.vault_root)
        .unwrap_or(&target_file_path)
        .to_string_lossy()
        .to_string();
    let result = parser::parse_content(&processed_content, &file_path_str)?;
    
    let writer = writer_arc.lock().unwrap();
    index::index_file(&writer, fields, &file_path_str, &result.chunks, &result.meta)?;

    Ok(())
}
