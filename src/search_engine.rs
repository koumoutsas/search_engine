use std::sync::Mutex;

use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tempfile::TempDir;

use crate::search::SearchResult;

pub trait Writer {
    fn write(&self, text: &str, url: &str, origin_url: &str, depth: u32);
}

pub trait Reader {
    fn read(&self, query: &str) -> Result<Vec<SearchResult>, String>;
}

pub struct SearchEngine {
    // Needed to prevent its destructor from removing the folder
    index_path: TempDir,
    index: Index,
    // Wrapping it with a mutex allows IndexWriter to be mutable and used cross-thread.
    // The underlying implementation is thread-safe, but cargo doesn't know that
    index_writer: Mutex<IndexWriter>,
    schema: Schema,
    reader: IndexReader
}

unsafe impl Send for SearchEngine {}
unsafe impl Sync for SearchEngine {}

impl Default for SearchEngine {
    fn default() -> Self {
        let index_path = TempDir::new().expect("Unable to create temp dir");
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("url", STRING | STORED);
        schema_builder.add_text_field("origin_url", STRING | STORED);
        schema_builder.add_u64_field("depth", STORED);
        schema_builder.add_text_field("body", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_dir(&index_path, schema.clone()).expect("Unable to create index");
        let index_writer = index.writer(50_000_000).expect("Unable to create writer");
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into().expect("Unable to create reader");
        Self {
            index_path,
            index,
            index_writer: Mutex::new(index_writer),
            schema,
            reader
        }
    }
}
impl Writer for SearchEngine {
    fn write(&self, text: &str, url: &str, origin_url: &str, depth: u32) {
        let url_field = self.schema.get_field("url").unwrap();
        let origin_url_field = self.schema.get_field("origin_url").unwrap();
        let depth_field = self.schema.get_field("depth").unwrap();
        let body_field = self.schema.get_field("body").unwrap();
        let mut guard = self.index_writer.lock().unwrap();
        match guard.add_document(doc!(
        url_field => url,
        origin_url_field => origin_url,
        depth_field => depth as u64,
        body_field => text
        )) {
                Ok(_) => {
                    match guard.commit() {
                        Ok(_) => {},
                        Err(e) => println!("Failed to index {}. Error: {}", url, e)
                    }
                }
                Err(e) => println!("Failed to index {}. Error: {}", url, e)
            }
    }
}

fn get_text_field_value(doc: &Document, field: Field) -> String {
    doc.get_first(field).unwrap().as_text().unwrap().to_string()
}

fn get_int_field_value(doc: &Document, field: Field) -> u32 {
    doc.get_first(field).unwrap().as_u64().unwrap() as u32
}

impl Reader for SearchEngine {
    fn read(&self, query: &str) -> Result<Vec<SearchResult>, String>{
        let url_field = self.schema.get_field("url").unwrap();
        let origin_url_field = self.schema.get_field("origin_url").unwrap();
        let depth_field = self.schema.get_field("depth").unwrap();
        let body_field = self.schema.get_field("body").unwrap();
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![body_field]);
        let query = match query_parser.parse_query(query) {
            Ok(r) => Ok(r),
            Err(e) => Err(e.to_string())
        }?;
        let top_docs = match searcher.search(&query, &TopDocs::with_limit(10)) {
            Ok(r) => Ok(r),
            Err(e) => Err(e.to_string())
        }?;
        Ok(top_docs.iter().map(|(_score, doc_address)| {
            if let Ok(retrieved) = searcher.doc(*doc_address) {
                Ok(SearchResult{
                    relevant_url: get_text_field_value(&retrieved, url_field),
                    origin_url: get_text_field_value(&retrieved, origin_url_field),
                    depth: get_int_field_value(&retrieved, depth_field)
                })
            } else {
                return Err(())
            }}).filter(|r| r.is_ok()).map(|r| r.unwrap()).collect())
    }
}
