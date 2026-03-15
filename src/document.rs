use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Reads a specific range of lines from a given file inside the vault.
/// If `start_line` or `end_line` are not provided, it reads from the beginning or to the end respectively.
pub fn read_document_range(
    vault_root: &Path,
    file_path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<String> {
    let mut requested = PathBuf::from(file_path);
    
    // Auto-append .md if no extension is provided
    if requested.extension().is_none() {
        requested.set_extension("md");
    }
    
    // Only allow paths relative to the vault
    if requested.is_absolute() || file_path.starts_with('/') {
        anyhow::bail!("Security violation: Only relative paths within the vault are allowed");
    }
    
    let target_path = vault_root.join(requested);

    let canonical = std::fs::canonicalize(&target_path)
        .with_context(|| format!("Could not resolve file path: {}", target_path.display()))?;

    // Strict security check: ensure the resolved absolute path is still inside the vault root.
    // canonicalize() also resolves the vault_root to absolute path, so we must canonicalize the root for a safe comparison.
    let canonical_root = std::fs::canonicalize(vault_root)
        .with_context(|| format!("Could not resolve vault root: {}", vault_root.display()))?;

    if !canonical.starts_with(&canonical_root) {
        anyhow::bail!("Security violation: File path is outside the vault");
    }

    let actual_path = canonical;

    let file = std::fs::File::open(&actual_path)
        .with_context(|| format!("Could not open file: {}", actual_path.display()))?;
    let reader = BufReader::new(file);

    let mut lines = Vec::new();
    let start = start_line.unwrap_or(1).max(1);
    
    for (i, line) in reader.lines().enumerate() {
        let current_line = i + 1;
        
        // Stop reading if we've passed the end line
        if let Some(end) = end_line {
            if current_line > end {
                break;
            }
        }
        
        if current_line >= start {
            if let Ok(l) = line {
                lines.push(format!("{:4} | {}", current_line, l));
            }
        }
    }

    Ok(lines.join("\n"))
}
