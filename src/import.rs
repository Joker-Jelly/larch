use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::VaultConfig;
use crate::index::{self, SchemaFields};
use crate::parser;

/// Maximum allowed content size for imports (10 MB).
const MAX_IMPORT_SIZE: usize = 10 * 1024 * 1024;

/// Result of a successful import operation.
pub struct ImportResult {
    /// Vault-relative path of the imported file.
    pub rel_path: String,
    /// Number of chunks indexed.
    pub chunks_indexed: usize,
}

/// Import markdown content into the vault: validate paths, process assets, write
/// the file, parse it, and index the resulting chunks.
///
/// This is the single source of truth for all import paths (CLI, REST API, MCP).
pub fn import_content(
    content: &str,
    filename: &str,
    dir: Option<&str>,
    config: &VaultConfig,
    fields: &SchemaFields,
    writer: &Arc<Mutex<tantivy::IndexWriter>>,
) -> Result<ImportResult> {
    // ── Input validation ────────────────────────────────────────────
    if content.len() > MAX_IMPORT_SIZE {
        anyhow::bail!(
            "Content too large ({} bytes, max {} bytes)",
            content.len(),
            MAX_IMPORT_SIZE
        );
    }

    // ── Target directory ────────────────────────────────────────────
    let target_dir = if let Some(d) = dir {
        config.vault_root.join(d)
    } else {
        config.vault_root.clone()
    };
    std::fs::create_dir_all(&target_dir).context("Creating target directory")?;

    let file_path = target_dir.join(filename);

    // ── Path traversal protection ───────────────────────────────────
    validate_within_vault(&file_path, &target_dir, &config.vault_root)?;

    // ── Asset processing ────────────────────────────────────────────
    let processed = crate::assets::process_assets(
        content,
        &target_dir,
        &config.vault_root,
        &target_dir,
    )
    .context("Processing assets")?;

    // ── Write file ──────────────────────────────────────────────────
    std::fs::write(&file_path, &processed)
        .with_context(|| format!("Writing file: {}", file_path.display()))?;

    // ── Parse & index ───────────────────────────────────────────────
    let rel_path = file_path
        .strip_prefix(&config.vault_root)
        .unwrap_or(&file_path)
        .to_string_lossy()
        .to_string();

    let result = parser::parse_content(&processed, &rel_path)
        .context("Parsing markdown")?;

    let chunks_count = result.chunks.len();

    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    index::index_file(&w, fields, &rel_path, &result.chunks, &result.meta)
        .context("Indexing")?;
    w.commit().context("Committing index")?;

    Ok(ImportResult {
        rel_path,
        chunks_indexed: chunks_count,
    })
}

/// Validate that `file_path` resolves to somewhere inside `vault_root`.
fn validate_within_vault(
    file_path: &Path,
    target_dir: &Path,
    vault_root: &Path,
) -> Result<()> {
    let canonical_vault =
        std::fs::canonicalize(vault_root).context("Resolving vault root")?;

    let canonical_file = if file_path.exists() {
        std::fs::canonicalize(file_path)?
    } else {
        let parent = file_path.parent().unwrap_or(target_dir);
        std::fs::canonicalize(parent)
            .context("Resolving parent directory")?
            .join(file_path.file_name().unwrap_or_default())
    };

    if !canonical_file.starts_with(&canonical_vault) {
        anyhow::bail!("Path escapes vault boundary");
    }
    Ok(())
}

/// Import a single file from disk into the vault (used by CLI `import` command).
///
/// Handles asset processing, copying/moving, and indexing — but does NOT commit
/// the writer (caller batches commits).
pub fn import_file_from_disk(
    file_path: &Path,
    source_base_dir: &Path,
    target_md_base: &Path,
    config: &VaultConfig,
    fields: &SchemaFields,
    writer: &Arc<Mutex<tantivy::IndexWriter>>,
    move_file: bool,
) -> Result<PathBuf> {
    let rel_path = file_path
        .strip_prefix(source_base_dir)
        .unwrap_or(Path::new(file_path.file_name().unwrap()));
    let target_file_path = target_md_base.join(rel_path);

    if let Some(parent) = target_file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Reading {}", file_path.display()))?;

    let processed = crate::assets::process_assets(
        &content,
        file_path.parent().unwrap_or(Path::new("")),
        &config.vault_root,
        target_file_path.parent().unwrap_or(Path::new("")),
    )?;

    std::fs::write(&target_file_path, &processed)
        .with_context(|| format!("Writing to {}", target_file_path.display()))?;

    if move_file {
        let _ = std::fs::remove_file(file_path);
    }

    let file_path_str = target_file_path
        .strip_prefix(&config.vault_root)
        .unwrap_or(&target_file_path)
        .to_string_lossy()
        .to_string();
    let result = parser::parse_content(&processed, &file_path_str)?;

    let w = writer.lock().unwrap_or_else(|e| e.into_inner());
    index::index_file(&w, fields, &file_path_str, &result.chunks, &result.meta)?;

    Ok(target_file_path)
}
