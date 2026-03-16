use std::collections::{HashMap, HashSet};
use tantivy::schema::{Field, Value};
use tantivy::Index;

pub fn get_all_tags(index: &Index, tags_field: Field) -> tantivy::Result<HashMap<String, Vec<String>>> {
    let reader = index
        .reader_builder()
        .try_into()?;
    let searcher = reader.searcher();
    
    let mut all_tags = HashSet::new();

    for segment_reader in searcher.segment_readers() {
        let inverted_index = segment_reader.inverted_index(tags_field)?;
        let mut term_stream = inverted_index.terms().stream()?;
        while let Some((term_bytes, _term_info)) = term_stream.next() {
            if let Ok(term_str) = std::str::from_utf8(term_bytes) {
                all_tags.insert(term_str.to_string());
            }
        }
    }

    let mut result = HashMap::new();
    let schema = index.schema();
    let file_path_field = schema.get_field("file_path").unwrap();

    // Now, for each tag, run a simple query to get the associated files
    // This is faster than reading the document store for every single document in the index
    for tag in all_tags {
        let tag_term = tantivy::Term::from_field_text(tags_field, &tag);
        let query = tantivy::query::TermQuery::new(tag_term, tantivy::schema::IndexRecordOption::Basic);
        
        let top_docs = searcher.search(&query, &tantivy::collector::TopDocs::with_limit(10_000))?;
        let mut paths = HashSet::new();
        
        for (_score, doc_addr) in top_docs {
            let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;
            if let Some(path) = doc.get_first(file_path_field).and_then(|v| v.as_str()) {
                paths.insert(path.to_string());
            }
        }
        
        let mut paths_vec: Vec<String> = paths.into_iter().collect();
        paths_vec.sort();
        result.insert(tag, paths_vec);
    }

    Ok(result)
}

pub fn get_files_for_tag(index: &Index, tags_field: Field, tag: &str) -> tantivy::Result<Vec<String>> {
    let reader = index.reader_builder().try_into()?;
    let searcher = reader.searcher();
    
    let tag_term = tantivy::Term::from_field_text(tags_field, tag);
    let query = tantivy::query::TermQuery::new(tag_term, tantivy::schema::IndexRecordOption::Basic);
    
    let top_docs = searcher.search(&query, &tantivy::collector::TopDocs::with_limit(10_000))?;
    
    let schema = index.schema();
    let file_path_field = schema.get_field("file_path").unwrap();
    
    let mut paths = HashSet::new();
    
    for (_score, doc_addr) in top_docs {
        let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;
        if let Some(path) = doc.get_first(file_path_field).and_then(|v| v.as_str()) {
            paths.insert(path.to_string());
        }
    }
    
    let mut result: Vec<String> = paths.into_iter().collect();
    result.sort();
    Ok(result)
}