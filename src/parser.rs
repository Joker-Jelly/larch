use anyhow::{Context, Result};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

// ── Compiled regexes (built once) ─────────────────────────────────

static INLINE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([^\s#]+)").unwrap());

// ── Data structures ────────────────────────────────────────────────

/// File-level metadata extracted from YAML frontmatter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileMeta {
    pub title: Option<String>,
    pub date: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub summary: Option<String>,
    pub version: Option<String>,
}

/// A logical chunk produced by parsing a Markdown file.
#[derive(Debug, Clone, Serialize)]
pub struct Chunk {
    pub chunk_id: String,
    pub file_path: String,
    pub title_hierarchy: String,
    pub content: String,
    pub start_line: u64,
    pub end_line: u64,
    pub tags: Vec<String>,
    pub keywords: Vec<String>,
}

/// Result of parsing a single Markdown file.
#[derive(Debug)]
pub struct ParseResult {
    pub meta: FileMeta,
    pub chunks: Vec<Chunk>,
}

// ── Helpers ────────────────────────────────────────────────────────

/// Build a lookup table: byte-offset → 1-based line number.
fn build_line_index(source: &str) -> Vec<usize> {
    let mut line_starts: Vec<usize> = vec![0]; // line 1 starts at byte 0
    for (i, ch) in source.bytes().enumerate() {
        if ch == b'\n' {
            line_starts.push(i + 1);
        }
    }
    line_starts
}

/// Given a byte offset and pre-computed line-start table, return 1-based line.
fn offset_to_line(line_starts: &[usize], offset: usize) -> u64 {
    match line_starts.binary_search(&offset) {
        Ok(idx) => (idx + 1) as u64,
        Err(idx) => idx as u64, // falls between two starts → belongs to previous line
    }
}

fn heading_level_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Extract inline hashtags from content (e.g. `#性能优化`).
fn extract_inline_tags(content: &str) -> Vec<String> {
    INLINE_TAG_RE
        .captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Generate a short chunk id from file path and start line.
fn make_chunk_id(file_path: &str, start_line: u64) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{:x}_{}", hash & 0xFFFF_FFFF, start_line)
}

// ── Frontmatter ────────────────────────────────────────────────────

/// Strip YAML frontmatter and return (meta, body, frontmatter_line_count).
fn parse_frontmatter(source: &str) -> (FileMeta, &str, usize) {
    if !source.starts_with("---") {
        return (FileMeta::default(), source, 0);
    }

    // Find the end of the frontmatter block
    let matter_end = if let Some(end_rel) = source[3..].find("---") {
        end_rel + 3 + 3 // +3 for the skipped part, +3 for the '---' itself
    } else {
        return (FileMeta::default(), source, 0);
    };

    let (frontmatter, body_ref) = source.split_at(matter_end);
    let fm_lines = frontmatter.matches('\n').count();

    let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
    let result = matter.parse(source);

    let meta: FileMeta = result
        .data
        .and_then(|d| {
            // gray_matter returns data as a Pod; convert via serde_json roundtrip
            let json = pod_to_json(&d);
            serde_json::from_value(json).ok()
        })
        .unwrap_or_default();

    (meta, body_ref, fm_lines)
}

/// Convert gray_matter Pod to serde_json Value.
fn pod_to_json(pod: &gray_matter::Pod) -> serde_json::Value {
    match pod {
        gray_matter::Pod::Null => serde_json::Value::Null,
        gray_matter::Pod::String(s) => serde_json::Value::String(s.clone()),
        gray_matter::Pod::Integer(i) => serde_json::json!(i),
        gray_matter::Pod::Float(f) => serde_json::json!(f),
        gray_matter::Pod::Boolean(b) => serde_json::Value::Bool(*b),
        gray_matter::Pod::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(pod_to_json).collect())
        }
        gray_matter::Pod::Hash(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), pod_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
    }
}

// ── Main parser ────────────────────────────────────────────────────

/// Parse a Markdown file into metadata + logical chunks.
pub fn parse_file(file_path: &Path) -> Result<ParseResult> {
    let source = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let file_path_str = file_path.to_string_lossy().to_string();
    parse_content(&source, &file_path_str)
}

/// Parse markdown content string; `file_path_str` is used for chunk IDs.
pub fn parse_content(source: &str, file_path_str: &str) -> Result<ParseResult> {
    let (meta, body, fm_line_offset) = parse_frontmatter(source);
    let line_starts = build_line_index(source);

    // pulldown-cmark parser with offsets (offsets are relative to `body`)
    let opts = Options::all();
    let parser = Parser::new_ext(body, opts);
    let events: Vec<(Event, std::ops::Range<usize>)> = parser.into_offset_iter().collect();

    // body starts at this byte position in `source`
    let body_byte_offset = source.len() - body.len();

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut title_stack: Vec<(usize, String)> = Vec::new(); // (depth, title)
    let mut current_content = String::new();
    let mut current_start_byte: Option<usize> = None;
    let mut in_heading = false;
    let mut pending_heading_text = String::new();
    let mut pending_heading_depth: usize = 0;

    let file_title = meta
        .title
        .clone()
        .unwrap_or_else(|| {
            Path::new(file_path_str)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| file_path_str.to_string())
        });

    for (event, range) in &events {
        let abs_byte = range.start + body_byte_offset;

        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Finalize the previous chunk (if any)
                if current_start_byte.is_some() || !current_content.trim().is_empty() {
                    let start_byte = current_start_byte.unwrap_or(body_byte_offset);
                    let end_byte = abs_byte.saturating_sub(1).max(start_byte);
                    finalize_chunk(
                        &mut chunks,
                        &current_content,
                        &title_stack,
                        &file_title,
                        file_path_str,
                        start_byte,
                        end_byte,
                        &line_starts,
                        &meta.tags,
                        &meta.keywords,
                    );
                    current_content.clear();
                    current_start_byte = None;
                }

                in_heading = true;
                pending_heading_depth = heading_level_depth(*level);
                pending_heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                // Update title stack
                while title_stack
                    .last()
                    .is_some_and(|(d, _)| *d >= pending_heading_depth)
                {
                    title_stack.pop();
                }
                title_stack.push((pending_heading_depth, pending_heading_text.clone()));
                current_start_byte = Some(abs_byte);
            }
            Event::Text(text) | Event::Code(text) => {
                if in_heading {
                    pending_heading_text.push_str(text);
                } else {
                    if current_start_byte.is_none() {
                        current_start_byte = Some(abs_byte);
                    }
                    current_content.push_str(text);
                    current_content.push(' ');
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if !in_heading {
                    current_content.push('\n');
                }
            }
            _ => {}
        }
    }

    // Finalize last chunk
    if !current_content.trim().is_empty() {
        let start_byte = current_start_byte.unwrap_or(body_byte_offset);
        let end_byte = source.len().saturating_sub(1).max(start_byte);
        finalize_chunk(
            &mut chunks,
            &current_content,
            &title_stack,
            &file_title,
            file_path_str,
            start_byte,
            end_byte,
            &line_starts,
            &meta.tags,
            &meta.keywords,
        );
    }

    // If nothing produced, create a single preamble chunk for the whole file
    if chunks.is_empty() && !body.trim().is_empty() {
        let start_line = (fm_line_offset + 1) as u64;
        let end_line = source.matches('\n').count() as u64 + 1;
        chunks.push(Chunk {
            chunk_id: make_chunk_id(file_path_str, start_line),
            file_path: file_path_str.to_string(),
            title_hierarchy: file_title.clone(),
            content: body.to_string(),
            start_line,
            end_line,
            tags: meta.tags.clone(),
            keywords: meta.keywords.clone(),
        });
    }

    Ok(ParseResult { meta, chunks })
}

#[allow(clippy::too_many_arguments)]
fn finalize_chunk(
    chunks: &mut Vec<Chunk>,
    content: &str,
    title_stack: &[(usize, String)],
    file_title: &str,
    file_path_str: &str,
    start_byte: usize,
    end_byte: usize,
    line_starts: &[usize],
    yaml_tags: &[String],
    yaml_keywords: &[String],
) {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return;
    }

    let start_line = offset_to_line(line_starts, start_byte);
    let end_line = offset_to_line(line_starts, end_byte);

    // Build title hierarchy
    let hierarchy = if title_stack.is_empty() {
        file_title.to_string()
    } else {
        title_stack
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" > ")
    };

    // Merge YAML tags + inline tags, deduplicate
    let inline_tags = extract_inline_tags(trimmed);
    let mut all_tags: HashSet<String> = yaml_tags.iter().cloned().collect();
    all_tags.extend(inline_tags);
    let mut tags: Vec<String> = all_tags.into_iter().collect();
    tags.sort();

    chunks.push(Chunk {
        chunk_id: make_chunk_id(file_path_str, start_line),
        file_path: file_path_str.to_string(),
        title_hierarchy: hierarchy,
        content: trimmed.to_string(),
        start_line,
        end_line,
        tags,
        keywords: yaml_keywords.to_vec(),
    });
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_heading_chunks() {
        let md = "# Title\n\nSome intro text\n\n## Section A\n\nContent A\n\n## Section B\n\nContent B\n";
        let result = parse_content(md, "test.md").unwrap();
        assert!(result.chunks.len() >= 2, "expected >=2 chunks, got {}", result.chunks.len());
    }

    #[test]
    fn test_no_headings_preamble() {
        let md = "Just some plain text without any headings.\n";
        let result = parse_content(md, "test.md").unwrap();
        assert_eq!(result.chunks.len(), 1);
        assert!(result.chunks[0].content.contains("plain text"));
    }

    #[test]
    fn test_frontmatter_parsing() {
        let md = r#"---
title: "Rust 并发模型"
tags: [rust, 并发]
summary: "RwLock usage"
version: "1.0"
---
# Heading

Body content
"#;
        let result = parse_content(md, "test.md").unwrap();
        assert_eq!(result.meta.title.as_deref(), Some("Rust 并发模型"));
        assert_eq!(result.meta.tags, vec!["rust", "并发"]);
        assert_eq!(result.meta.summary.as_deref(), Some("RwLock usage"));
        assert_eq!(result.meta.version.as_deref(), Some("1.0"));
    }

    #[test]
    fn test_inline_tag_extraction() {
        let tags = extract_inline_tags("Some text #性能优化 more #架构设计 end");
        assert!(tags.contains(&"性能优化".to_string()));
        assert!(tags.contains(&"架构设计".to_string()));
    }

    #[test]
    fn test_cjk_content() {
        let md = "# 中文标题\n\n这是一段中文正文内容。\n";
        let result = parse_content(md, "test.md").unwrap();
        assert!(!result.chunks.is_empty());
        let has_chinese = result.chunks.iter().any(|c| c.content.contains("中文"));
        assert!(has_chinese, "Chinese content should be preserved");
    }

    #[test]
    fn test_title_hierarchy() {
        let md = "# Top\n\n## Sub\n\n### Sub-Sub\n\nDeep content\n";
        let result = parse_content(md, "test.md").unwrap();
        let deep = result.chunks.iter().find(|c| c.content.contains("Deep")).unwrap();
        assert!(deep.title_hierarchy.contains("Top"));
        assert!(deep.title_hierarchy.contains("Sub"));
    }
}