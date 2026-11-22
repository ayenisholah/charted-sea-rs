// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The README quick start, against a mock server. If this breaks, the README is wrong.

mod common;

use charted_sea::{Scraper, TaskRequest, WaitOptions};
use common::{Sequence, client, fixture, task};
use futures::StreamExt;
use serde_json::json;
use std::time::Duration;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

#[tokio::test]
async fn readme_quick_start_runs() -> Result<(), charted_sea::Error> {
    let server = MockServer::start().await;
    let submitted = fixture("run-scraper-params").response.body;
    let uuid = submitted[0]["uuid"].as_str().unwrap().to_owned();
    Mock::given(matchers::method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(submitted))
        .mount(&server)
        .await;
    Mock::given(matchers::method("GET"))
        .respond_with(Sequence::json(vec![
            json!([task(&uuid, "RUNNING")]),
            json!([task(&uuid, "SUCCESS")]),
        ]))
        .mount(&server)
        .await;
    let client = client(&server);
    let options = WaitOptions::default()
        .poll_interval(Duration::from_millis(1))
        .max_poll_interval(Duration::from_millis(2));

    // --- README ---
    let tasks = client.tasks(Scraper::Shopee);
    let submitted = tasks
        .run([TaskRequest::get(
            "https://shopee.sg/api/v4/search/search_items?keyword=tshirt",
        )])
        .await?;
    let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();
    let mut done = Vec::new();
    for task in tasks.wait_for_results(&uuids, options).await? {
        done.push(format!("{} {}", task.url, task.status));
    }
    let mut finished = tasks.watch(&uuids, options);
    while let Some(task) = finished.next().await {
        done.push(task?.status.to_string());
    }
    // --- end README ---

    assert_eq!(
        done,
        [
            format!("https://shopee.sg/i/{uuid} SUCCESS"),
            "SUCCESS".to_owned()
        ]
    );
    Ok(())
}
