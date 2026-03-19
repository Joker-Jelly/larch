use std::collections::{HashMap, HashSet};
use tantivy::schema::Value;

use crate::index::SchemaFields;

pub fn get_all_tags(index: &tantivy::Index, fields: &SchemaFields) -> tantivy::Result<HashMap<String, Vec<String>>> {
    let reader = index
        .reader_builder()
        .try_into()?;
    let searcher = reader.searcher();

    let mut all_tags = HashSet::new();

    for segment_reader in searcher.segment_readers() {
        let inverted_index = segment_reader.inverted_index(fields.tags)?;
        let mut term_stream = inverted_index.terms().stream()?;
        while let Some((term_bytes, _term_info)) = term_stream.next() {
            if let Ok(term_str) = std::str::from_utf8(term_bytes) {
                all_tags.insert(term_str.to_string());
            }
        }
    }

    let mut result = HashMap::new();

    for tag in all_tags {
        let tag_term = tantivy::Term::from_field_text(fields.tags, &tag);
        let query = tantivy::query::TermQuery::new(tag_term, tantivy::schema::IndexRecordOption::Basic);

        let top_docs = searcher.search(&query, &tantivy::collector::TopDocs::with_limit(10_000))?;
        let mut paths = HashSet::new();

        for (_score, doc_addr) in top_docs {
            let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;
            if let Some(path) = doc.get_first(fields.file_path).and_then(|v| v.as_str()) {
                paths.insert(path.to_string());
            }
        }

        let mut paths_vec: Vec<String> = paths.into_iter().collect();
        paths_vec.sort();
        result.insert(tag, paths_vec);
    }

    Ok(result)
}

pub fn get_files_for_tag(index: &tantivy::Index, fields: &SchemaFields, tag: &str) -> tantivy::Result<Vec<String>> {
    let reader = index.reader_builder().try_into()?;
    let searcher = reader.searcher();

    let tag_term = tantivy::Term::from_field_text(fields.tags, tag);
    let query = tantivy::query::TermQuery::new(tag_term, tantivy::schema::IndexRecordOption::Basic);

    let top_docs = searcher.search(&query, &tantivy::collector::TopDocs::with_limit(10_000))?;

    let mut paths = HashSet::new();

    for (_score, doc_addr) in top_docs {
        let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;
        if let Some(path) = doc.get_first(fields.file_path).and_then(|v| v.as_str()) {
            paths.insert(path.to_string());
        }
    }

    let mut result: Vec<String> = paths.into_iter().collect();
    result.sort();
    Ok(result)
}
