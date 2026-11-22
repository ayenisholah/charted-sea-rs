// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Health, proxies and paging against the documented examples (FR-1, FR-6, FR-7).

mod common;

use charted_sea::{
    Error, Granularity, HealthQuery, ProxyConfig, ProxySetting, ReadQuery, Scraper, TimeBound,
};
use common::{client, fixture, serve_fixture, task};
use futures::StreamExt;
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

fn documented_health_query() -> HealthQuery {
    HealthQuery::new(
        TimeBound::Last24h,
        Granularity::Hour,
        [Scraper::Shopee],
        ["SG", "ID"],
    )
}

#[tokio::test]
async fn fr_1_session_metrics_match_the_docs() {
    let server = MockServer::start().await;
    serve_fixture(&server, "health-session-metrics").await;
    let report = client(&server)
        .health()
        .session_metrics(&documented_health_query())
        .await
        .unwrap();
    assert_eq!(report.periods.len(), 2);
    let id = report.metrics().find(|m| m.country == "ID").unwrap();
    assert_eq!(
        (id.preparation_success_rate, id.p95_proxy_bytes),
        (Some(0.72), Some(9_234_000.0))
    );
    assert_eq!(report.extra["countries"], json!(["SG", "ID"]));
}

#[tokio::test]
async fn fr_1_task_metrics_send_known_apis_and_return_per_endpoint_rows() {
    let server = MockServer::start().await;
    serve_fixture(&server, "health-task-metrics").await;
    let query = documented_health_query().known_apis(["PRODUCT_SEARCH", "PRODUCT_DETAIL"]);
    let report = client(&server).health().task_metrics(&query).await.unwrap();
    let rows: Vec<String> = report
        .metrics()
        .map(|m| {
            format!(
                "{}/{}:{}",
                m.country,
                m.known_api.as_deref().unwrap_or("-"),
                m.success_rate.unwrap_or_default()
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            "SG/PRODUCT_SEARCH:0.97",
            "SG/PRODUCT_DETAIL:0.94",
            "ID/PRODUCT_SEARCH:0.65",
            "ID/UNKNOWN:0.88"
        ]
    );
}

#[tokio::test]
async fn fr_7_invalid_health_queries_send_nothing() {
    let server = MockServer::start().await;
    let health = client(&server).health();
    let empty = HealthQuery::new(TimeBound::Last1h, Granularity::Total, [], ["SG"]);
    assert!(matches!(
        health.session_metrics(&empty).await,
        Err(Error::Validation { .. })
    ));
    assert!(matches!(
        health.task_metrics(&empty).await,
        Err(Error::Validation { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn fr_1_validate_posts_the_url_array() {
    let server = MockServer::start().await;
    serve_fixture(&server, "proxy-validate").await;
    let urls: Vec<String> =
        serde_json::from_value(fixture("proxy-validate").request.body.unwrap()).unwrap();
    let results = client(&server).proxies().validate(&urls).await.unwrap();
    let tcp = results[0]
        .tests
        .iter()
        .find(|t| t.kind == "tcp20443")
        .unwrap();
    assert_eq!(
        (tcp.success, tcp.errors.as_slice()),
        (false, ["Error: read ECONNRESET".to_owned()].as_slice())
    );
    let shopee = &results[1].compatibilities[0];
    assert_eq!(
        (shopee.compatible, shopee.incompatibility_reason.as_deref()),
        (false, Some("Unsupported protocol/port(s): HTTP3/443"))
    );
    assert!(
        !format!("{results:?}").contains("ppp"),
        "Debug hides proxy credentials"
    );
}

#[tokio::test]
async fn fr_1_read_and_update_round_trip_the_documented_configuration() {
    let server = MockServer::start().await;
    serve_fixture(&server, "proxy-configs-read").await;
    serve_fixture(&server, "proxy-configs-update").await;
    let proxies = client(&server).proxies();
    let configs = proxies.read().await.unwrap();
    assert_eq!(
        configs
            .iter()
            .map(|c| c.scraper.as_str())
            .collect::<Vec<_>>(),
        ["SHOPEE", "SHOPEE_ANDROID", "LAZADA"]
    );
    assert_eq!(configs[0].proxy_settings[1].country.as_deref(), Some("TW"));
    assert!(
        !format!("{configs:?}").contains("ppp"),
        "Debug hides proxy credentials"
    );
    let updated = proxies.update(&configs).await.unwrap();
    assert_eq!(
        serde_json::to_value(updated).unwrap(),
        fixture("proxy-configs-update").response.body
    );
}

#[tokio::test]
async fn an_invalid_update_returns_invalid_urls_in_details_once() {
    let server = MockServer::start().await;
    serve_fixture(&server, "proxy-configs-update-invalid").await;
    let configs = [ProxyConfig::new(
        "SHOPEE",
        [ProxySetting::new("RESIDENTIAL", ["socks5://..."])],
    )];
    let error = client(&server)
        .proxies()
        .update(&configs)
        .await
        .unwrap_err();
    let Error::Validation {
        details: Some(details),
        ..
    } = error
    else {
        panic!("{error:?}")
    };
    assert_eq!(
        details,
        fixture("proxy-configs-update-invalid").response.body
    );
    let setting = ProxySetting::new("DATACENTER", ["http://u:secret@h:1"]).country("TW");
    assert!(format!("{setting:?}").contains("http://***@h:1"));
}

#[tokio::test]
async fn validate_refuses_an_empty_list() {
    let server = MockServer::start().await;
    let empty: Vec<String> = Vec::new();
    assert!(matches!(
        client(&server).proxies().validate(&empty).await,
        Err(Error::Validation { .. })
    ));
}

#[tokio::test]
async fn fr_6_pages_yield_every_task_once_and_stop_on_a_short_page() {
    let server = MockServer::start().await;
    let all: Vec<Value> = (0..45u128)
        .map(|i| task(&uuid::Uuid::from_u128(i + 1).to_string(), "SUCCESS"))
        .collect();
    let pages = all.clone();
    Mock::given(matchers::method("GET"))
        .respond_with(move |req: &wiremock::Request| {
            let get = |k: &str| {
                req.url
                    .query_pairs()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.parse::<usize>().unwrap())
                    .unwrap()
            };
            let (offset, limit) = (get("offset"), get("limit"));
            // The second page overlaps the first by one task, as if a task arrived meanwhile.
            let from = if offset == 20 { 19 } else { offset };
            ResponseTemplate::new(200)
                .set_body_json(json!({ "items": pages[from.min(45)..(from + limit).min(45)] }))
        })
        .mount(&server)
        .await;
    let tasks: Vec<_> = client(&server)
        .tasks(Scraper::Shopee)
        .pages(ReadQuery::new())
        .map(|t| t.unwrap().uuid.unwrap())
        .collect()
        .await;
    let mut unique = tasks.clone();
    unique.dedup();
    assert_eq!(tasks.len(), unique.len(), "no task twice");
    assert_eq!(tasks.len(), 44);
    let offsets: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "offset")
                .unwrap()
                .1
                .into_owned()
        })
        .collect();
    assert_eq!(offsets, ["0", "20", "40"]);
}

#[tokio::test]
async fn pages_end_on_an_error_and_keep_tasks_without_uuid() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .respond_with(common::Sequence::new(vec![
            ResponseTemplate::new(200).set_body_json(
                json!([{ "url": "a", "status": "PENDING" }, { "url": "b", "status": "PENDING" }]),
            ),
            ResponseTemplate::new(401),
        ]))
        .mount(&server)
        .await;
    let items: Vec<_> = client(&server)
        .tasks(Scraper::Lazada)
        .pages(ReadQuery::new().limit(2).offset(4))
        .collect()
        .await;
    assert_eq!(items.len(), 3);
    assert!(matches!(items[2], Err(Error::Auth { .. })));
}
