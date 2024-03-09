use crate::crawly::CrawlerBuilder;
use crate::search::SearchResult;
use crate::search_engine::{Reader, SearchEngine};

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
        crawler.start(origin_url.to_string(), &self.search_engine).await?;
        Ok(())
    }
}

impl Reader for IndexerService {
    fn read(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        self.search_engine.read(query)
    }
}
