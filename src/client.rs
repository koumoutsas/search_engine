use tonic::{Request, Response};
use crate::search::{IndexRequest, IndexResponse, ResponseStatus, SearchRequest, SearchResponse, SearchResult};
use crate::search::searcher_client::SearcherClient;

mod search {
    include!("search.rs");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = SearcherClient::connect("http://[::1]:50051").await?;
    let origin_url = "https://www.example.com";
    handle_index_result(client.index(Request::new(IndexRequest {
        origin: origin_url.to_string(),
        k: 4,
    })).await?, origin_url)?;
    let query = "example domain";
    handle_query_result(client.search(Request::new(SearchRequest {
        query: query.to_string()
    })).await?, query)?;
    Ok(())
}

fn print(results: &Vec<SearchResult>) {
    for result in results {
        println!("relevant URL: {}, origin URL: {}, depth: {}", result.relevant_url, result.origin_url, result.depth);
    }
}

fn handle_index_result(response: Response<IndexResponse>, origin_url: &str) -> Result<(), String> {
    match response.get_ref().status() {
        ResponseStatus::Ok => {
            println!("Successfully indexed {}", origin_url);
            Ok(())
        },
        ResponseStatus::Error => {
            Err(format!("Failed to index {}. Error {}", origin_url, response.get_ref().message()))
        }
    }
}

fn handle_query_result(response: Response<SearchResponse>, query: &str) -> Result<(), String> {
    match response.get_ref().status() {
        ResponseStatus::Ok => {
            println!("Query {} returned results:", query);
            print(&response.get_ref().results);
            Ok(())
        },
        ResponseStatus::Error => Err(format!("Query {} failed. Error {}", query, response.get_ref().message()))
    }
}
