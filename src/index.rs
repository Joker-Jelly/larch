use anyhow::{Context, Result};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexWriter, ReloadPolicy, TantivyDocument};

use crate::parser::{Chunk, FileMeta};

// ── Schema field names ─────────────────────────────────────────────

const F_FILE_PATH: &str = "file_path";
const F_CHUNK_ID: &str = "chunk_id";
const F_TITLE_HIERARCHY: &str = "title_hierarchy";
const F_CONTENT: &str = "content";
const F_START_LINE: &str = "start_line";
const F_END_LINE: &str = "end_line";
const F_KEYWORDS: &str = "keywords";
const F_TAGS: &str = "tags";
const F_DIRS: &str = "dirs";
const F_SUMMARY: &str = "summary";
const F_VERSION: &str = "version";
const F_CREATED_AT: &str = "created_at";

// Tokenizer name for jieba
const JIEBA_TOKENIZER: &str = "jieba";

// ── Public types ───────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub title_hierarchy: String,
    pub content_snippet: String,
    pub start_line: u64,
    pub end_line: u64,
    pub score: f32,
    pub summary: String,
    pub keywords: String,
    pub tags: Vec<String>,
}

/// Holds field references to avoid repeated schema lookups.
#[derive(Clone)]
pub struct SchemaFields {
    pub file_path: Field,
    pub chunk_id: Field,
    pub title_hierarchy: Field,
    pub content: Field,
    pub start_line: Field,
    pub end_line: Field,
    pub keywords: Field,
    pub tags: Field,
    pub dirs: Field,
    pub summary: Field,
    pub version: Field,
    pub created_at: Field,
}

// ── Index operations ───────────────────────────────────────────────

/// Build the Tantivy schema.
fn build_schema() -> (Schema, SchemaFields) {
    let mut builder = Schema::builder();

    let file_path = builder.add_text_field(F_FILE_PATH, STRING | STORED);
    let chunk_id = builder.add_text_field(F_CHUNK_ID, STRING | STORED);
    let title_hierarchy = builder.add_text_field(
        F_TITLE_HIERARCHY,
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(JIEBA_TOKENIZER)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored(),
    );
    let content = builder.add_text_field(
        F_CONTENT,
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(JIEBA_TOKENIZER)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored(),
    );
    let start_line = builder.add_u64_field(F_START_LINE, INDEXED | STORED);
    let end_line = builder.add_u64_field(F_END_LINE, INDEXED | STORED);
    let keywords = builder.add_text_field(
        F_KEYWORDS,
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(JIEBA_TOKENIZER)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored(),
    );
    let tags = builder.add_text_field(
        F_TAGS,
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            )
            .set_stored()
            .set_fast(None),
    );
    let dirs = builder.add_facet_field(F_DIRS, INDEXED);
    let summary = builder.add_text_field(F_SUMMARY, STORED);
    let version = builder.add_text_field(F_VERSION, STRING | STORED);
    let created_at = builder.add_text_field(
        F_CREATED_AT,
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            )
            .set_stored(),
    );

    let schema = builder.build();
    let fields = SchemaFields {
        file_path,
        chunk_id,
        title_hierarchy,
        content,
        start_line,
        end_line,
        keywords,
        tags,
        dirs,
        summary,
        version,
        created_at,
    };
    (schema, fields)
}

/// Open or create a Tantivy index at `index_dir`, registering the jieba tokenizer.
pub fn open_or_create(index_dir: &Path) -> Result<(Index, SchemaFields)> {
    let (schema, fields) = build_schema();

    std::fs::create_dir_all(index_dir)?;

    let index = if index_dir.join("meta.json").exists() {
        Index::open_in_dir(index_dir).context("opening existing index")?
    } else {
        Index::create_in_dir(index_dir, schema).context("creating new index")?
    };

    // Register the jieba tokenizer
    let tokenizer = tantivy_jieba::JiebaTokenizer {};
    index.tokenizers().register(JIEBA_TOKENIZER, tokenizer);

    Ok((index, fields))
}

/// Create an index writer with a reasonable heap budget.
pub fn create_writer(index: &Index) -> Result<IndexWriter> {
    index
        .writer(50_000_000) // 50 MB heap
        .context("creating index writer")
}

/// Index (or re-index) a single file's chunks.
/// Deletes any previously-indexed chunks for the same `file_path` first.
pub fn index_file(
    writer: &IndexWriter,
    fields: &SchemaFields,
    file_path: &str,
    chunks: &[Chunk],
    meta: &FileMeta,
) -> Result<()> {
    // Delete existing documents for this file
    let file_path_term = tantivy::Term::from_field_text(fields.file_path, file_path);
    writer.delete_term(file_path_term);

    let summary_str = meta.summary.as_deref().unwrap_or("");
    let version_str = meta.version.as_deref().unwrap_or("");
    let created_at_str = meta.date.as_deref().unwrap_or("");

    for chunk in chunks {
        let mut doc = TantivyDocument::new();
        doc.add_text(fields.file_path, file_path);
        doc.add_text(fields.chunk_id, &chunk.chunk_id);
        doc.add_text(fields.title_hierarchy, &chunk.title_hierarchy);
        doc.add_text(fields.content, &chunk.content);
        doc.add_u64(fields.start_line, chunk.start_line);
        doc.add_u64(fields.end_line, chunk.end_line);
        doc.add_text(fields.keywords, chunk.keywords.join(" "));
        
        for tag in &chunk.tags {
            doc.add_text(fields.tags, tag);
        }

        let path = std::path::Path::new(file_path);
        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().replace('\\', "/");
            let facet_path = if parent_str.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", parent_str)
            };
            doc.add_facet(fields.dirs, tantivy::schema::Facet::from(&facet_path));
        } else {
            doc.add_facet(fields.dirs, tantivy::schema::Facet::from("/"));
        }

        doc.add_text(fields.summary, summary_str);
        doc.add_text(fields.version, version_str);
        doc.add_text(fields.created_at, created_at_str);
        writer.add_document(doc)?;
    }

    Ok(())
}

/// Remove all indexed chunks for a file.
pub fn remove_file(writer: &IndexWriter, fields: &SchemaFields, file_path: &str) {
    let term = tantivy::Term::from_field_text(fields.file_path, file_path);
    writer.delete_term(term);
}

/// Execute a search query and return results.
pub fn search(
    index: &Index,
    fields: &SchemaFields,
    query_str: &str,
    tag: Option<&str>,
    dir: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .context("creating reader")?;

    let searcher = reader.searcher();

    let mut query_parser = QueryParser::for_index(
        index,
        vec![fields.content, fields.title_hierarchy, fields.keywords],
    );
    query_parser.set_field_boost(fields.keywords, 3.0);
    query_parser.set_field_boost(fields.title_hierarchy, 2.0);
    query_parser.set_field_boost(fields.content, 1.0);

    let parsed_query = query_parser
        .parse_query(query_str)
        .context("parsing query")?;

    let mut boolean_queries: Vec<(tantivy::query::Occur, Box<dyn tantivy::query::Query>)> = vec![
        (tantivy::query::Occur::Must, parsed_query.box_clone()),
    ];

    if let Some(t) = tag {
        let tag_term = tantivy::Term::from_field_text(fields.tags, t);
        let tag_query = Box::new(tantivy::query::TermQuery::new(
            tag_term,
            tantivy::schema::IndexRecordOption::Basic,
        ));
        boolean_queries.push((tantivy::query::Occur::Must, tag_query));
    }

    if let Some(d) = dir {
        let facet_path = if d.starts_with('/') {
            d.to_string()
        } else {
            format!("/{}", d)
        };
        let facet = tantivy::schema::Facet::from(&facet_path);
        let dir_term = tantivy::Term::from_facet(fields.dirs, &facet);
        let dir_query = Box::new(tantivy::query::TermQuery::new(
            dir_term,
            tantivy::schema::IndexRecordOption::Basic,
        ));
        boolean_queries.push((tantivy::query::Occur::Must, dir_query));
    }

    let final_query = tantivy::query::BooleanQuery::new(boolean_queries);

    let top_docs = searcher
        .search(&final_query, &TopDocs::with_limit(limit))
        .context("executing search")?;

    let snippet_generator = tantivy::SnippetGenerator::create(&searcher, &*parsed_query, fields.content)?;
    
    let mut results = Vec::new();

    for (score, doc_addr) in top_docs {
        let doc: TantivyDocument = searcher.doc(doc_addr).context("retrieving doc")?;

        let get_text = |field: Field| -> String {
            doc.get_first(field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let get_u64 = |field: Field| -> u64 {
            doc.get_first(field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        };

        let mut tags = Vec::new();
        for value in doc.get_all(fields.tags) {
            if let Some(s) = value.as_str() {
                tags.push(s.to_string());
            }
        }

        // Build a content snippet
        let snippet = snippet_generator.snippet_from_doc(&doc);
        
        let content_snippet = if snippet.is_empty() {
            let full_content = get_text(fields.content);
            if full_content.chars().count() > 200 {
                let truncated: String = full_content.chars().take(200).collect();
                format!("{}...", truncated)
            } else {
                full_content
            }
        } else {
            // The easiest and safest way is to use `to_html` and replace the tags with ANSI codes
            // Because Tantivy correctly handles byte boundaries for multi-byte UTF-8 chars in `to_html`.
            // We also decode basic HTML entities escaped by Tantivy's encode_minimal.
            snippet
                .to_html()
                .replace("<b>", "\x1b[1;31m")  // Bold Red for highlight
                .replace("</b>", "\x1b[0m")    // Reset
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace('\n', " ")
        };

        results.push(SearchResult {
            chunk_id: get_text(fields.chunk_id),
            file_path: get_text(fields.file_path),
            title_hierarchy: get_text(fields.title_hierarchy),
            content_snippet,
            start_line: get_u64(fields.start_line),
            end_line: get_u64(fields.end_line),
            score,
            summary: get_text(fields.summary),
            keywords: get_text(fields.keywords),
            tags,
        });
    }

    Ok(results)
}

/// Return total number of documents (chunks) in the index.
pub fn doc_count(index: &Index) -> Result<u64> {
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let searcher = reader.searcher();
    Ok(searcher.num_docs())
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chunks() -> (FileMeta, Vec<Chunk>) {
        let meta = FileMeta {
            title: Some("Test Doc".into()),
            summary: Some("A test document".into()),
            tags: vec!["test".into()],
            keywords: vec!["meta_kw".into()],
            version: Some("1.0".into()),
            date: Some("2026-01-01".into()),
        };
        let chunks = vec![
            Chunk {
                chunk_id: "abc_1".into(),
                file_path: "test.md".into(),
                title_hierarchy: "Test Doc".into(),
                content: "Rust 并发模型 解析 RwLock".into(),
                start_line: 1,
                end_line: 5,
                tags: vec!["rust".into(), "并发".into()],
                keywords: vec!["meta_kw".into()],
            },
            Chunk {
                chunk_id: "abc_6".into(),
                file_path: "test.md".into(),
                title_hierarchy: "Test Doc > Details".into(),
                content: "详细的性能优化指南 performance guide".into(),
                start_line: 6,
                end_line: 10,
                tags: vec!["性能".into()],
                keywords: vec!["meta_kw".into()],
            },
        ];
        (meta, chunks)
    }

    #[test]
    fn test_index_and_search() {
        let tmp = std::env::temp_dir().join("larch_index_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let (index, fields) = open_or_create(&tmp).unwrap();
        let mut writer = create_writer(&index).unwrap();

        let (meta, chunks) = sample_chunks();
        index_file(&writer, &fields, "test.md", &chunks, &meta).unwrap();
        writer.commit().unwrap();

        // Search for Chinese content
        let results = search(&index, &fields, "并发", None, None, 10).unwrap();
        assert!(!results.is_empty(), "Should find Chinese content");
        assert_eq!(results[0].file_path, "test.md");

        // Search for English content
        let results = search(&index, &fields, "performance", None, None, 10).unwrap();
        assert!(!results.is_empty(), "Should find English content");

        // Search by tag
        let results = search(&index, &fields, "performance", Some("性能"), None, 10).unwrap();
        assert!(!results.is_empty(), "Should find content with tag");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_remove_file() {
        let tmp = std::env::temp_dir().join("larch_remove_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let (index, fields) = open_or_create(&tmp).unwrap();
        let mut writer = create_writer(&index).unwrap();

        let (meta, chunks) = sample_chunks();
        index_file(&writer, &fields, "test.md", &chunks, &meta).unwrap();
        writer.commit().unwrap();

        // Remove and verify
        remove_file(&writer, &fields, "test.md");
        writer.commit().unwrap();

        let results = search(&index, &fields, "并发", None, None, 10).unwrap();
        assert!(results.is_empty(), "Should be empty after removal");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
