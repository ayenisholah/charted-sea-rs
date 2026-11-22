//! Submit search pages and stream each finished task as one JSON line on stdout.
//!
//! `CHARTED_SEA_TOKEN=... cargo run --example stream_to_jsonl -- tshirt hoodie > tasks.jsonl`

use std::time::Duration;

use charted_sea::{Client, Scraper, TaskRequest, WaitOptions};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let keywords: Vec<String> = std::env::args().skip(1).collect();
    if keywords.is_empty() {
        return Err("usage: stream_to_jsonl <keyword>...".into());
    }
    let client = Client::from_env()?;
    let tasks = client.tasks(Scraper::Shopee);
    let requests = keywords.iter().map(|k| {
        TaskRequest::get(format!(
            "https://shopee.sg/api/v4/search/search_items?keyword={k}"
        ))
    });
    // Cached results up to an hour old are free.
    let submitted = tasks
        .run(requests)
        .cache_max_age(Duration::from_secs(3600))
        .await?;
    let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();

    let mut finished = tasks.watch(&uuids, WaitOptions::default());
    while let Some(task) = finished.next().await {
        println!("{}", serde_json::to_string(&task?)?);
    }
    Ok(())
}
