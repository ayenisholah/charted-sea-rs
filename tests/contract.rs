// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Every request matches a documented example exactly (FR-1), using the fixtures shared with charted-sea-client.

mod common;

use std::time::Duration;

use charted_sea::{
    CancelRequest, CountQuery, Error, ReadQuery, Scraper, TaskRequest, TaskStatus, Uuid,
};
use common::{client, fixture, serve_fixture};
use serde_json::{Value, json};
use time::macros::datetime;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

fn documented_requests() -> Vec<TaskRequest> {
    let body = fixture("run-sync").request.body.unwrap();
    body["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| match r.get("payload") {
            Some(p) => TaskRequest::post(r["url"].as_str().unwrap(), p.clone()),
            None => TaskRequest::get(r["url"].as_str().unwrap()),
        })
        .collect()
}

#[tokio::test]
async fn fr_2_run_is_asynchronous_by_default_and_matches_the_docs() {
    let server = MockServer::start().await;
    serve_fixture(&server, "run-async").await;
    let tasks = client(&server)
        .tasks(Scraper::Shopee)
        .run(documented_requests())
        .auto_cancel_after(Duration::from_secs(1200))
        .cache_max_age(Duration::from_secs(3600))
        .await
        .unwrap();
    assert!(
        tasks
            .iter()
            .all(|t| t.status == TaskStatus::Pending && t.uuid.is_some())
    );
}

#[tokio::test]
async fn fr_1_run_with_wait_returns_task_outcomes_as_data() {
    let server = MockServer::start().await;
    serve_fixture(&server, "run-sync").await;
    let tasks = client(&server)
        .tasks("shopee")
        .run(documented_requests())
        .wait(true)
        .auto_cancel_after(Duration::from_secs(1200))
        .cache_max_age(Duration::from_secs(3600))
        .await
        .unwrap();
    let statuses: Vec<_> = tasks.iter().map(|t| t.status.clone()).collect();
    assert_eq!(
        statuses,
        [
            TaskStatus::Success,
            TaskStatus::ErrorTooManyTimes,
            TaskStatus::BlockedTooManyTimes
        ]
    );
    assert_eq!(
        tasks[1].error_message.as_deref(),
        Some("... explanation ...")
    );
}

#[tokio::test]
async fn fr_9_scraper_params_go_to_the_body_root() {
    let server = MockServer::start().await;
    serve_fixture(&server, "run-scraper-params").await;
    client(&server)
        .tasks(Scraper::Shopee)
        .run([TaskRequest::get(
            "https://shopee.co.id/api/v4/search/search_items?keyword=tshirt",
        )])
        .scraper_param("productSearch_enrichUrlQuery_pageSize", json!(60))
        .scraper_param("productSearch_crawlNextPages", json!(true))
        .scraper_param("productSearch_crawlNextPages_maxPages", json!(3))
        .scraper_param("productSearch_crawlProductDetails", json!(true))
        .await
        .unwrap();
}

#[tokio::test]
async fn fr_1_run_single_puts_the_request_at_the_root() {
    let server = MockServer::start().await;
    serve_fixture(&server, "run-single-sync").await;
    let url = fixture("run-single-sync").request.body.unwrap()["url"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = client(&server)
        .tasks(Scraper::Shopee)
        .run_single(TaskRequest::get(url))
        .wait(true)
        .auto_cancel_after(Duration::from_secs(1200))
        .await
        .unwrap();
    assert_eq!(task.status, TaskStatus::Success);
}

#[tokio::test]
async fn fr_1_read_count_and_cancel_match_the_docs() {
    let server = MockServer::start().await;
    for name in ["read-tasks", "count-tasks", "cancel-tasks"] {
        serve_fixture(&server, name).await;
    }
    let tasks = client(&server).tasks(Scraper::Shopee);
    let uuids: Vec<Uuid> = fixture("read-tasks").request.query["uuids"]
        .split(',')
        .map(|u| u.parse().unwrap())
        .collect();
    let read = tasks
        .read(ReadQuery::new().uuids(uuids.clone()).limit(20).offset(0))
        .await
        .unwrap();
    assert_eq!(
        read.iter().map(|t| t.uuid.unwrap()).collect::<Vec<_>>(),
        uuids
    );

    let statuses = [
        TaskStatus::Pending,
        TaskStatus::Running,
        TaskStatus::Blocked,
        TaskStatus::Error,
    ];
    let counts = tasks
        .count(
            CountQuery::new()
                .statuses(statuses)
                .min_pending_at(datetime!(2026-07-01 00:00:00 UTC)),
        )
        .await
        .unwrap();
    assert_eq!(counts.len(), 3);
    assert_eq!(counts[1].count, 452_746_545);

    let body = fixture("cancel-tasks").request.body.unwrap();
    let request = CancelRequest::new()
        .uuids(
            body["uuids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|u| u.as_str().unwrap().parse::<Uuid>().unwrap()),
        )
        .countries(
            body["countries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap().to_owned()),
        );
    assert_eq!(tasks.cancel(request).await.unwrap().uuids.len(), 5);
}

#[tokio::test]
async fn fr_1_read_accepts_the_page_object_the_live_api_returns() {
    let server = MockServer::start().await;
    serve_fixture(&server, "read-tasks-live").await;
    let uuid: Uuid = "27eb6795-7e90-4928-9f13-cbb753f4563c".parse().unwrap();
    let tasks = client(&server)
        .tasks(Scraper::Shopee)
        .read(ReadQuery::new().uuids([uuid]).limit(1))
        .await
        .unwrap();
    assert_eq!(tasks[0].status, TaskStatus::Blocked);
}

#[tokio::test]
async fn fr_5_batches_more_than_100_uuids_and_merges_in_input_order() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/scraping-tasks/shopee"))
        .respond_with(|req: &wiremock::Request| {
            let asked: Vec<String> = req
                .url
                .query_pairs()
                .find(|(k, _)| k == "uuids")
                .map(|(_, v)| v.split(',').map(str::to_owned).collect())
                .unwrap_or_default();
            // Reverse order and drop one, to prove the merge.
            let tasks: Vec<Value> = asked
                .iter()
                .rev()
                .filter(|u| !u.ends_with("042"))
                .map(|u| common::task(u, "SUCCESS"))
                .collect();
            ResponseTemplate::new(200)
                .set_body_json(json!({ "items": tasks, "total": tasks.len() }))
        })
        .mount(&server)
        .await;
    let uuids: Vec<Uuid> = (0..250u128)
        .map(|i| format!("00000000-0000-4000-8000-{i:012}").parse().unwrap())
        .collect();
    let mut with_dupe = uuids.clone();
    with_dupe.push(uuids[0]);
    let tasks = client(&server)
        .tasks(Scraper::Shopee)
        .read(ReadQuery::new().uuids(with_dupe))
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let limits: Vec<String> = requests
        .iter()
        .map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "limit")
                .unwrap()
                .1
                .into_owned()
        })
        .collect();
    assert_eq!(limits, ["100", "100", "50"]);
    let expected: Vec<Uuid> = uuids
        .iter()
        .copied()
        .filter(|u| !u.to_string().ends_with("042"))
        .collect();
    assert_eq!(
        tasks.iter().map(|t| t.uuid.unwrap()).collect::<Vec<_>>(),
        expected
    );
}

#[tokio::test]
async fn reads_with_urls_dates_fields_and_without_uuids() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/scraping-tasks/lazada"))
        .and(common::ExactQuery(
            [
                ("urls", "https://a,https://b"),
                ("minPendingAt", "2026-07-01T00:00:00Z"),
                ("maxPendingAt", "2026-07-02T00:00:00Z"),
                ("includeAllFields", "true"),
                ("includeFields", "uuid,url"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let tasks = client(&server).tasks(Scraper::Lazada);
    let query = ReadQuery::new()
        .urls(["https://a", "https://b"])
        .min_pending_at(datetime!(2026-07-01 00:00:00 UTC))
        .max_pending_at(datetime!(2026-07-02 00:00:00 UTC))
        .include_all_fields()
        .include_fields(["uuid", "url"]);
    assert!(tasks.read(query).await.unwrap().is_empty());
    assert!(
        tasks
            .read(ReadQuery::new().uuids([]))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn run_options_reach_the_query_string() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/scraping-tasks/shopee-android/run"))
        .and(common::ExactQuery(
            [
                ("waitForCompletion", "false"),
                ("includeAllFields", "true"),
                ("includeFields", "uuid"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    client(&server)
        .tasks(Scraper::ShopeeAndroid)
        .run([TaskRequest::get("u")])
        .include_all_fields()
        .include_fields(["uuid"])
        .await
        .unwrap();
}

#[tokio::test]
async fn local_checks_fail_before_any_request() {
    let server = MockServer::start().await;
    let shopee = client(&server).tasks(Scraper::Shopee);
    let many: Vec<Uuid> = (0..101u128).map(Uuid::from_u128).collect();
    let errors = [
        shopee.run([]).await.unwrap_err(),
        shopee
            .run([TaskRequest::get("u")])
            .scraper_param("requests", json!([]))
            .await
            .unwrap_err(),
        shopee.cancel(CancelRequest::new()).await.unwrap_err(),
        shopee
            .read(ReadQuery::new().uuids(many).limit(5))
            .await
            .unwrap_err(),
        client(&server)
            .tasks(" ")
            .count(CountQuery::new())
            .await
            .unwrap_err(),
    ];
    assert!(
        errors
            .iter()
            .all(|e| matches!(e, Error::Validation { endpoint, .. } if endpoint == "client")),
        "{errors:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancel_sends_only_what_was_given() {
    let server = MockServer::start().await;
    let a: Uuid = "2199a83d-3ebe-442c-aa82-afd3a9903967".parse().unwrap();
    for body in [json!({ "uuids": [a] }), json!({ "countries": ["ID"] })] {
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/scraping-tasks/shopee/cancel"))
            .and(matchers::body_json(&body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let tasks = client(&server).tasks(Scraper::Shopee);
    assert!(
        tasks
            .cancel(CancelRequest::new().uuids([a]))
            .await
            .unwrap()
            .uuids
            .is_empty()
    );
    assert!(
        tasks
            .cancel(CancelRequest::new().countries(["ID"]))
            .await
            .unwrap()
            .countries
            .is_empty()
    );
}
