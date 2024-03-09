//! The `Crawly` web crawler efficiently fetches and stores content from web pages.
//! It respects `robots.txt` guidelines and handles rate limits.

use anyhow::Result;
use futures::future::join_all;
use indexmap::IndexMap;
pub use mime::Mime;
use reqwest::header::HeaderValue;
use reqwest::{Client, Url};
use robotstxt::DefaultMatcher;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::fmt::Debug;
use std::str::FromStr;
use tokio::sync::{RwLock, Semaphore};
use tokio::time::{sleep, Duration};
use crate::search_engine::Writer;

const USER_AGENT: &str = "CrawlyRustCrawler";

// Default configuration constants.
const MAX_DEPTH: usize = 5;
const MAX_PAGES: usize = 15;
const MAX_CONCURRENT_REQUESTS: usize = 1_000;
const RATE_LIMIT_WAIT_SECONDS: u64 = 1;

/// Cache structure to store information about a domain's `robots.txt`.
#[derive(Debug)]
struct RobotsCache {
    content: String,
    crawl_delay: Option<u64>, // Delay specified by the `robots.txt`.
}

/// Configuration parameters for the `Crawler`.
/// Defines bounds and behaviors for the crawling process.
struct CrawlerConfig {
    user_agent: String,
    max_depth: usize,
    max_pages: usize,
    max_concurrent_requests: usize,
    rate_limit_wait_seconds: u64,
    robots: bool,
    allowed_mimes: Vec<Mime>,
}

impl Default for CrawlerConfig {
    /// Default configuration for the crawler.
    fn default() -> Self {
        Self {
            user_agent: USER_AGENT.into(),
            max_depth: MAX_DEPTH,
            max_pages: MAX_PAGES,
            max_concurrent_requests: MAX_CONCURRENT_REQUESTS,
            rate_limit_wait_seconds: RATE_LIMIT_WAIT_SECONDS,
            robots: true,
            allowed_mimes: vec![],
        }
    }
}

/// Builder pattern for `Crawler`. Allows for customizable configurations.
pub struct CrawlerBuilder {
    config: CrawlerConfig,
}

impl Default for CrawlerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CrawlerBuilder {
    /// Initializes a new builder with default configuration.
    pub fn new() -> Self {
        CrawlerBuilder {
            config: CrawlerConfig::default(),
        }
    }

    /// Set a specific maximum depth for the crawl.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.config.max_depth = depth;
        self
    }

    /// Set a specific maximum number of pages to fetch.
    pub fn with_max_pages(mut self, pages: usize) -> Self {
        self.config.max_pages = pages;
        self
    }

    /// Set a limit for concurrent requests.
    pub fn with_max_concurrent_requests(mut self, requests: usize) -> Self {
        self.config.max_concurrent_requests = requests;
        self
    }

    /// Define a rate limit delay in seconds.
    pub fn with_rate_limit_wait_seconds(mut self, seconds: u64) -> Self {
        self.config.rate_limit_wait_seconds = seconds;
        self
    }

    /// Enable or disable `robots.txt` handling
    pub fn with_robots(mut self, robots: bool) -> Self {
        self.config.robots = robots;
        self
    }

    /// Set a custom user agent
    pub fn with_user_agent<S: AsRef<str>>(mut self, user_agent: S) -> Self {
        self.config.user_agent = user_agent.as_ref().into();
        self
    }

    /// Allow only a set of MIMEs
    pub fn with_allowed_mimes(mut self, mime_types: Vec<Mime>) -> Self {
        self.config.allowed_mimes = mime_types;
        self
    }

    /// Consumes the builder and returns a configured `Crawler` instance.
    pub fn build(self) -> Result<Crawler> {
        Crawler::from_config(self.config)
    }
}

/// Main structure for the `Crawler` containing necessary utilities and caches.
pub struct Crawler {
    config: CrawlerConfig, // Configuration parameters.
    client: Client,        // HTTP client to make web requests.
    robots_cache: RwLock<IndexMap<String, RobotsCache>>, // Cache for `robots.txt` per domain.
}

impl Crawler {
    /// Initializes the crawler with a given configuration.
    fn from_config(config: CrawlerConfig) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent(config.user_agent.as_str())
                .build()?,
            robots_cache: RwLock::new(IndexMap::new()),
            config,
        })
    }

    /// Initializes a new `Crawler` instance with the default configuration.
    pub fn new() -> Result<Self> {
        Self::from_config(CrawlerConfig::default())
    }

    /// Asynchronously crawls a URL. Honors `robots.txt`, maintains state about visited URLs,
    /// and manages rate limits and concurrency.
    #[async_recursion::async_recursion]
    #[tracing::instrument(skip(self, semaphore, visited, writer))]
    async fn crawl(
        &self,
        semaphore: &Semaphore, // Rate limiting and concurrency management.
        origin_url: &str,
        url: Url,
        depth: usize,                            // Current depth of the crawl.
        visited: &RwLock<HashSet<Url>>,          // Set of visited URLs to avoid redundancy.
        writer: &(dyn Writer + Send + Sync)
    ) -> Result<()> {
        let permit = semaphore.acquire().await;
        // Recursion base cases.
        if depth > self.config.max_depth
            || visited.read().await.len() > self.config.max_pages
            || visited.read().await.contains(&url)
        {
            tracing::info!(
                "Reached the limit {{ depth: {depth}, visited: {} }}.",
                visited.read().await.len()
            );

            return Ok(());
        }

        let domain = url.domain().unwrap_or_default().to_string();

        if self.config.robots {
            // Fetch and handle `robots.txt` for the domain.
            let robots_url = format!(
                "{}://{}/robots.txt",
                url.scheme(),
                url.host().ok_or(anyhow::anyhow!("Host not found."))?
            );

            let mut robots_cache = self.robots_cache.write().await;

            // Get cached robots info or fetch if not cached.
            let robots = if let Some(info) = robots_cache.get(&domain) {
                tracing::debug!(
                    "Cache found for robots.txt {{ robots_cache: {robots_cache:#?} }}."
                );

                Some((
                    info.content.clone(),
                    info.crawl_delay.unwrap_or(RATE_LIMIT_WAIT_SECONDS),
                ))
            } else if let Ok(response) = self.client.get(&robots_url).send().await {
                let robots_content = response.text().await?;

                tracing::debug!("Cache not found for robots.txt, fetched a new one {{ robots_content: {robots_content} }}.");

                let delay_seconds = robots_content
                    .lines()
                    .filter_map(|line| {
                        if line.contains("Crawl-delay") {
                            line.split(':').last()?.trim().parse().ok()
                        } else {
                            None
                        }
                    })
                    .next()
                    .unwrap_or(RATE_LIMIT_WAIT_SECONDS);

                robots_cache.insert(
                    domain.clone(),
                    RobotsCache {
                        content: robots_content.clone(),
                        crawl_delay: Some(delay_seconds),
                    },
                );

                Some((robots_content, delay_seconds))
            } else {
                None
            };

            drop(robots_cache);

            if let Some((robots_content, delay_seconds)) = robots {
                tracing::debug!("Sleeping for {delay_seconds} due to robots.txt policies...");

                // Respect the crawl delay specified by `robots.txt`.
                sleep(Duration::from_secs(delay_seconds)).await;

                // Check permission from `robots.txt` before proceeding.
                if !DefaultMatcher::default().one_agent_allowed_by_robots(
                    &robots_content,
                    self.config.user_agent.as_str(),
                    url.as_str(),
                ) {
                    return Ok(());
                }
            }
        } else {
            sleep(Duration::from_secs(self.config.rate_limit_wait_seconds)).await;
        }

        let response = self.client.get(url.clone()).send().await?;

        // Check if the response is mitigated by Cloudflare and skip it
        if response.headers().get("cf-mitigated") == Some(&HeaderValue::from_str("challenge")?) {
            tracing::debug!("Cloudflare mitigation found, skipping this URL {{ url: {url} }}");

            return Ok(());
        }

        // Fetch the page content.
        let page = response.bytes().await?.to_vec();

        if !self.config.allowed_mimes.is_empty()
            && infer::get(page.as_slice())
            .map(|mime| {
                if let Ok(mime) = Mime::from_str(mime.mime_type()) {
                    self.config.allowed_mimes.contains(&mime)
                } else {
                    true
                }
            })
            .unwrap_or(true)
        {
            // Explicitly dropping the permit to free up concurrency slot.
            drop(permit);

            visited.write().await.insert(url.clone());

            return Ok(());
        }

        // Fetch the page content.
        let url_content = String::from_utf8(page)?;
        writer.write(&url_content.clone(), &url.clone().to_string(), origin_url, depth as u32);

        // Explicitly dropping the permit to free up concurrency slot.
        drop(permit);

        visited.write().await.insert(url.clone());

        // Continue crawling by processing extracted links recursively.
        let _ = join_all(
            Self::extract_links(url_content.as_str())
                .map(|links| {
                    tracing::debug!(
                        "Found other sub-URLs {{ len: {}, links: {links:#?} }}",
                        links.len()
                    );

                    links
                })?
                .into_iter()
                .filter_map(|link| match url.join(&link) {
                    Ok(url) => {
                        if url.domain().unwrap_or_default() == domain {
                            Some(self.crawl(semaphore, origin_url, url, depth + 1, visited, writer))
                        } else {
                            None
                        }
                    },
                    Err(_) => None,
                }),
        )
            .await;

        tracing::info!("Finished crawling URL {{ url: {url} }}");

        Ok(())
    }

    /// Extracts hyperlinks from given HTML content.
    #[tracing::instrument(skip(content))]
    fn extract_links(content: &str) -> Result<Vec<String>> {
        let document = Html::parse_document(content);
        let selector = Selector::parse("a").map_err(|error| anyhow::anyhow!("{:?}", error))?;

        Ok(document
            .select(&selector)
            .filter_map(|element| element.value().attr("href").map(|href| href.to_string()))
            .collect())
    }

    /// Initiates the crawling process from a specified root URL.
    ///
    /// Returns a map of visited URLs and their corresponding HTML content.
    #[tracing::instrument(skip(self, writer))]
    pub async fn start<S: AsRef<str> + Debug>(&self, url: S, writer: &(dyn Writer + Send + Sync)) -> Result<()> {
        let root_url = Url::parse(url.as_ref())?;

        let semaphore = Semaphore::new(self.config.max_concurrent_requests);
        let visited = RwLock::new(HashSet::new());

        self.crawl(&semaphore, &root_url.clone().to_string(), root_url, 0, &visited, writer)
            .await?;

        Ok(())
    }
}
