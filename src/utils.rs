use std::path::Path;

/// Check if a given path is a Markdown file.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "md" || ext == "markdown")
}