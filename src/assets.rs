use anyhow::{Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

/// Process a markdown file's content, copying any locally-referenced assets
/// into `<vault_root>/assets/` with hash-based filenames, then rewriting the
/// markdown paths to use vault-relative references.
///
/// Returns the rewritten markdown content.
pub fn process_assets(
    md_content: &str,
    source_dir: &Path,
    vault_root: &Path,
    target_md_dir: &Path,
) -> Result<String> {
    let assets_dir = vault_root.join("assets");
    std::fs::create_dir_all(&assets_dir).context("creating assets dir")?;

    // Regex for ![alt](path) and [text](path) — avoids URLs (http/https)
    let re = Regex::new(r"(!?\[[^\]]*\])\(([^)]+)\)").unwrap();

    let mut replacements: HashMap<String, String> = HashMap::new();

    for cap in re.captures_iter(md_content) {
        let original_path_str = cap[2].trim();

        // Skip URLs and already-vault-relative paths
        if original_path_str.starts_with("http://")
            || original_path_str.starts_with("https://")
            || original_path_str.starts_with('#')
        {
            continue;
        }

        // Skip if already pointing to assets/
        if original_path_str.contains("assets/") {
            continue;
        }

        // Resolve to absolute path relative to the source markdown's directory
        let original_path = source_dir.join(original_path_str);

        if !original_path.exists() {
            // Asset file not found — leave the link as-is
            continue;
        }

        if replacements.contains_key(original_path_str) {
            continue;
        }

        // Hash the file content
        let hashed_name = hash_and_copy(&original_path, &assets_dir)?;

        // Compute relative path from target_md_dir to assets_dir
        let rel_path = pathdiff_relative(target_md_dir, &assets_dir);
        let new_ref = format!("{}/{}", rel_path, hashed_name);

        replacements.insert(original_path_str.to_string(), new_ref);
    }

    // Apply replacements
    let mut result = md_content.to_string();
    for (old, new) in &replacements {
        result = result.replace(old, new);
    }

    Ok(result)
}

/// Copy a file to `assets_dir` with a hash-based name.
/// Returns the new filename (not full path).
fn hash_and_copy(source: &Path, assets_dir: &Path) -> Result<String> {
    let content = std::fs::read(source).with_context(|| format!("reading asset {}", source.display()))?;

    let mut hasher = Sha256::new();
    hasher.update(&content);
    let hash = hasher.finalize();
    let hash_hex = format!("{:x}", hash);
    let short_hash = &hash_hex[..8];

    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "bin".to_string());

    // Prefix based on extension category
    let prefix = match ext.to_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => "img",
        "pdf" => "doc",
        "mp4" | "webm" | "mov" | "avi" => "vid",
        "mp3" | "wav" | "ogg" | "flac" => "aud",
        _ => "file",
    };

    let new_name = format!("{}_{}.{}", prefix, short_hash, ext);
    let dest = assets_dir.join(&new_name);

    if !dest.exists() {
        std::fs::copy(source, &dest)
            .with_context(|| format!("copying asset to {}", dest.display()))?;
    }

    Ok(new_name)
}

/// Compute a simple relative path string from `from_dir` to `to_dir`.
fn pathdiff_relative(from_dir: &Path, to_dir: &Path) -> String {
    // Try to compute relative path; fall back to assets/ if complex
    if let Ok(from_canon) = std::fs::canonicalize(from_dir) {
        if let Ok(to_canon) = std::fs::canonicalize(to_dir) {
            let from_components: Vec<_> = from_canon.components().collect();
            let to_components: Vec<_> = to_canon.components().collect();

            // Find common prefix length
            let common = from_components
                .iter()
                .zip(to_components.iter())
                .take_while(|(a, b)| a == b)
                .count();

            let ups = from_components.len() - common;
            let downs: Vec<_> = to_components[common..]
                .iter()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();

            let mut parts = vec!["..".to_string(); ups];
            parts.extend(downs);
            return parts.join("/");
        }
    }
    // Fallback
    "../assets".to_string()
}

/// Hash a file and return its short hash (first 8 hex chars of SHA256).
pub fn hash_file(path: &Path) -> Result<String> {
    let content = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let hash = hasher.finalize();
    Ok(format!("{:x}", hash)[..8].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_asset_rewrite() {
        let tmp = std::env::temp_dir().join("larch_asset_test");
        let _ = fs::remove_dir_all(&tmp);
        let source_dir = tmp.join("source");
        let vault_root = tmp.join("vault");
        let target_md_dir = vault_root.join("notes");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_md_dir).unwrap();
        fs::create_dir_all(vault_root.join("assets")).unwrap();

        // Create a fake image
        fs::write(source_dir.join("diagram.png"), b"fake png content").unwrap();

        let md = "# Test\n\n![架构图](diagram.png)\n";
        let result = process_assets(md, &source_dir, &vault_root, &target_md_dir).unwrap();

        // The path should have been rewritten
        assert!(!result.contains("diagram.png") || result.contains("assets/"));
        // Alt text preserved
        assert!(result.contains("架构图"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_hash_dedup() {
        let tmp = std::env::temp_dir().join("larch_hash_test");
        let _ = fs::remove_dir_all(&tmp);
        let assets_dir = tmp.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();

        let source = tmp.join("img.png");
        fs::write(&source, b"identical content").unwrap();

        let name1 = hash_and_copy(&source, &assets_dir).unwrap();
        let name2 = hash_and_copy(&source, &assets_dir).unwrap();
        assert_eq!(name1, name2, "Same content should get same hash filename");

        // Only one file in assets dir
        let count = fs::read_dir(&assets_dir).unwrap().count();
        assert_eq!(count, 1);

        let _ = fs::remove_dir_all(&tmp);
    }
}
