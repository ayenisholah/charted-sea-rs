// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Live smoke test against the real API. Spends at most one task credit the first time
//! (cached for a day). Run by hand: `CHARTED_SEA_TOKEN=... cargo test --test live -- --ignored`.

use std::time::Duration;

use charted_sea::{Client, Scraper, TaskRequest, WaitOptions};

#[tokio::test]
#[ignore = "calls the real API; needs CHARTED_SEA_TOKEN"]
async fn live_run_and_wait() {
    let client = Client::from_env().expect("CHARTED_SEA_TOKEN");
    let tasks = client.tasks(Scraper::Shopee);
    let submitted = tasks
        .run([TaskRequest::get(
            "https://shopee.sg/api/v4/search/search_items?keyword=tshirt",
        )])
        .cache_max_age(Duration::from_secs(86_400))
        .await
        .expect("run");
    let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();
    assert_eq!(uuids.len(), 1, "async run returns a uuid: {submitted:?}");
    let done = tasks
        .wait_for_results(
            &uuids,
            WaitOptions::default().max_wait(Duration::from_secs(600)),
        )
        .await
        .expect("wait");
    println!(
        "final: {} ({} chars)",
        done[0].status,
        done[0].response_body.as_deref().map_or(0, str::len)
    );
    assert!(done[0].status.is_terminal());
}
