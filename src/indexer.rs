use std::thread;
use crate::crawly::CrawlerBuilder;

use crate::search::SearchResult;
use crate::search_engine::{Reader, SearchEngine, Writer};

pub trait Indexer {
    async fn visit(&self, url: &str, max_depth: u32) -> anyhow::Result<()>;
}

pub struct IndexerService {
    search_engine: SearchEngine,
}

impl Default for IndexerService {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::default(),
        }
    }
}

impl Indexer for IndexerService {
    async fn visit(&self, origin_url: &str, max_depth: u32) -> anyhow::Result<()> {
        let crawler = CrawlerBuilder::new()
            .with_max_depth(max_depth as usize)
            .with_max_pages(3)
            .with_max_concurrent_requests(2)
            .with_robots(true)
            .build()?;
        let url_copy = origin_url.to_string();
        let t = thread::spawn(move || async move {
            let r = crawler.start(url_copy).await;
            r
        });
        let results = t.join().unwrap().await?;
        for result in results {
            self.search_engine.write(&result.1.0, result.0.as_str(), origin_url, result.1.1 as u32);
        }
        Ok(())
    }
}

impl Reader for IndexerService {
    fn read(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        self.search_engine.read(query)
    }
}
