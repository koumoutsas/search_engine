use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use reqwest::Url;
use voyager::{Collector, Crawler, CrawlerConfig, Response, Scraper};
use voyager::scraper::Selector;
use crate::search::SearchResult;

use crate::search_engine::{Reader, SearchEngine, Writer};

pub trait Indexer {
    async fn visit(&self, url: &str, max_depth: u32) -> Result<String, String>;
}

pub struct Explorer {
    /// visited urls mapped with all the urls that link to that url
    visited: HashMap<Url, HashSet<Url>>,
    link_selector: Selector,
}
impl Default for Explorer {
    fn default() -> Self {
        Self {
            visited: Default::default(),
            link_selector: Selector::parse("a").unwrap(),
        }
    }
}

impl Scraper for Explorer {
    type Output = (usize, Url, String);
    type State = Url;

    fn scrape(
        &mut self,
        mut response: Response<Self::State>,
        crawler: &mut Crawler<Self>,
    ) -> anyhow::Result<Option<Self::Output>> {
        if let Some(origin) = response.state.take() {
            self.visited
                .entry(response.response_url.clone())
                .or_default()
                .insert(origin);
        }

        for link in response.html().select(&self.link_selector) {
            if let Some(href) = link.value().attr("href") {
                if let Ok(url) = response.response_url.join(href) {
                    crawler.visit_with_state(url, response.response_url.clone());
                }
            }
        }

        Ok(Some((response.depth, response.response_url, response.text)))
    }
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

unsafe impl Send for IndexerService {}

impl Indexer for IndexerService {
    async fn visit(&self, origin_url: &str, max_depth: u32) -> Result<String, String> {
        let config = CrawlerConfig::default()
            .disallow_domains(vec!["facebook.com", "google.com"])
            .max_depth(max_depth as usize)
            .max_concurrent_requests(1_000);
        let mut collector = Collector::new(Explorer::default(), config);
        collector.crawler_mut().visit(origin_url);
        while let Ok(output) = collector.next().await.ok_or("Something went wrong with the scraper") {
            if let Ok((depth, url, text)) = output {
                self.search_engine.write(&text, url.as_str(), origin_url, depth as u32)
            }
        }
        Ok(format!("Completed crawl for {} at max depth {}", origin_url, max_depth))
    }
}

impl Reader for IndexerService {
    fn read(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        self.search_engine.read(query)
    }
}