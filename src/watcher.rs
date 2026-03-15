use anyhow::Result;
use notify_debouncer_full::{new_debouncer, notify::RecursiveMode, DebouncedEvent};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::VaultConfig;
use crate::index::{self, SchemaFields};
use crate::parser;

/// Events that the watcher translates into for the indexing pipeline.
#[derive(Debug)]
pub enum VaultEvent {
    Changed(PathBuf),
    Removed(PathBuf),
}

/// Start watching the vault directory. Returns a channel receiver for vault events,
/// and a join handle for the background thread.
pub fn start_watcher(
    config: &VaultConfig,
) -> Result<mpsc::UnboundedReceiver<VaultEvent>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let vault_root = config.vault_root.clone();
    let larch_dir = config.larch_dir();

    // Spawn a std thread for the blocking notify watcher
    std::thread::spawn(move || {
        let tx_clone = tx.clone();
        let larch_dir_clone = larch_dir.clone();

        let mut debouncer = match new_debouncer(
            Duration::from_secs(2), // Increased to 2 seconds to absorb multiple quick writes
            None,
            move |result: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| {
                match result {
                    Ok(events) => {
                        let mut processed_paths = std::collections::HashSet::new();
                        for event in events {
                            // Filter out Access (read) events to prevent infinite loops when we read the file
                            match event.kind {
                                notify::EventKind::Modify(_) |
                                notify::EventKind::Create(_) |
                                notify::EventKind::Remove(_) => {},
                                _ => continue,
                            }
                            
                            for path in &event.paths {
                                // Exclude larch internal directory and non-markdown files
                                if path.starts_with(&larch_dir_clone) || !is_markdown(path) {
                                    continue;
                                }
                                
                                // Prevent processing the exact same path multiple times in one debounce window
                                if !processed_paths.insert(path.clone()) {
                                    continue;
                                }

                                let vault_event = if path.exists() {
                                    VaultEvent::Changed(path.clone())
                                } else {
                                    VaultEvent::Removed(path.clone())
                                };
                                let _ = tx_clone.send(vault_event);
                            }
                        }
                    }
                    Err(errors) => {
                        for e in errors {
                            warn!("watcher error: {}", e);
                        }
                    }
                }
            },
        ) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to create watcher: {}", e);
                return;
            }
        };

        if let Err(e) = debouncer.watch(&vault_root, RecursiveMode::Recursive) {
            warn!("Failed to watch {}: {}", vault_root.display(), e);
            return;
        }

        info!("Watching vault: {}", vault_root.display());

        // Keep the thread alive so the watcher stays active
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });

    Ok(rx)
}

/// Process vault events: re-index changed files, remove deleted ones.
pub async fn process_events(
    mut rx: mpsc::UnboundedReceiver<VaultEvent>,
    _index: tantivy::Index,
    fields: SchemaFields,
    writer: Arc<Mutex<tantivy::IndexWriter>>,
    config: VaultConfig,
) {
    while let Some(event) = rx.recv().await {
        match event {
            VaultEvent::Changed(path) => {
                info!("Re-indexing: {}", path.display());
                let source = match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Could not read file {}: {}", path.display(), e);
                        continue;
                    }
                };
                let rel_path_str = path
                    .strip_prefix(&config.vault_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                
                match parser::parse_content(&source, &rel_path_str) {
                    Ok(result) => {
                        let mut w = writer.lock().unwrap();
                        if let Err(e) = index::index_file(&w, &fields, &rel_path_str, &result.chunks, &result.meta) {
                            warn!("Index error for {}: {}", path.display(), e);
                        }
                        if let Err(e) = w.commit() {
                            warn!("Commit error: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Parse error for {}: {}", path.display(), e);
                    }
                }
            }
            VaultEvent::Removed(path) => {
                info!("Removing from index: {}", path.display());
                let rel_path_str = path
                    .strip_prefix(&config.vault_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let mut w = writer.lock().unwrap();
                index::remove_file(&w, &fields, &rel_path_str);
                if let Err(e) = w.commit() {
                    warn!("Commit error: {}", e);
                }
            }
        }
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext == "md" || ext == "markdown")
        .unwrap_or(false)
}
