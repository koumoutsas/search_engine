use tonic::{Request, Response, Status};
use tonic::transport::Server;
use tracing_subscriber::{filter, Layer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use indexer::{Indexer, IndexerService};
use search::{IndexRequest, IndexResponse, ResponseStatus, SearchRequest, SearchResponse};
use search::searcher_server::{Searcher, SearcherServer};
use search_engine::Reader;

mod indexer;
mod search_engine;
mod client;
mod crawly;

mod search {
    include!("search.rs");
}

pub struct SearchService {
    indexer: Box<IndexerService>,
}

#[tonic::async_trait]
impl Searcher for SearchService {
    async fn index(&self, request: Request<IndexRequest>) -> Result<Response<IndexResponse>, Status> {
        let index_request = request.get_ref();
        let origin = &index_request.origin;
        let depth = &index_request.k;
        match self.indexer.visit(origin, *depth).await {
            Ok(()) => Ok(Response::new(IndexResponse {
                status: ResponseStatus::Ok.into(),
                message: None
            })),
            Err(error) => Err(Status::aborted(error.to_string()))
        }
    }

    async fn search(&self, request: Request<SearchRequest>) -> Result<Response<SearchResponse>, Status> {
        let query =  &request.get_ref().query;
        match self.indexer.read(query) {
            Ok(results) => Ok(Response::new(SearchResponse {
                status: ResponseStatus::Ok.into(),
                message: None,
                results
            })),
            Err(message) => Err(Status::aborted(message))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();
    tracing_subscriber::registry()
        .with(stdout_log.with_filter(filter::LevelFilter::INFO))
        .init();
    let addr = "[::1]:50051".parse().unwrap();
    let service = SearchService {
        indexer: Box::new(IndexerService::default())
    };
    println!("Search engine service listening on {}", addr);
    Server::builder()
        .add_service(SearcherServer::new(service))
        .serve(addr)
        .await?;
    Ok(())
}
