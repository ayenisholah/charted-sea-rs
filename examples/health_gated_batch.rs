//! Check recent platform health before spending credits on a batch, as the API docs recommend.
//!
//! `CHARTED_SEA_TOKEN=... cargo run --example health_gated_batch`

use std::time::Duration;

use charted_sea::{Client, Granularity, HealthQuery, Scraper, TaskRequest, TimeBound, WaitOptions};

const MIN_SUCCESS_RATE: f64 = 0.8;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::from_env()?;

    let query = HealthQuery::new(
        TimeBound::Last1h,
        Granularity::Total,
        [Scraper::Shopee],
        ["SG"],
    )
    .known_apis(["PRODUCT_SEARCH"]);
    let health = client.health().task_metrics(&query).await?;
    let mut healthy = false;
    for m in health.metrics() {
        eprintln!(
            "{} {:?} {:?}: {:?} ({:?})",
            m.country, m.device, m.known_api, m.success_rate, m.confidence
        );
        healthy |= m.confidence.as_deref() == Some("HIGH")
            && m.success_rate.is_some_and(|r| r >= MIN_SUCCESS_RATE);
    }
    if !healthy {
        eprintln!("Success rate below {MIN_SUCCESS_RATE} or low confidence; try again later.");
        std::process::exit(2);
    }

    let tasks = client.tasks(Scraper::Shopee);
    let submitted = tasks
        .run([TaskRequest::get(
            "https://shopee.sg/api/v4/search/search_items?keyword=tshirt",
        )])
        .cache_max_age(Duration::from_secs(3600))
        .await?;
    let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();
    for task in tasks
        .wait_for_results(&uuids, WaitOptions::default())
        .await?
    {
        println!("{} {}", task.url, task.status);
    }
    Ok(())
}
