// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `watch` and `wait_for_results` against a scripted server, with millisecond poll intervals.

mod common;

use std::time::Duration;

use charted_sea::{Error, Scraper, TaskStatus, Uuid, WaitOptions};
use common::{Sequence, client, task};
use futures::StreamExt;
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

const A: &str = "398a3b4d-241c-4297-9e81-5c4c6ecce340";
const B: &str = "785ebeaf-ae2c-4453-908b-c7dac008fbf4";
const C: &str = "2199a83d-3ebe-442c-aa82-afd3a9903967";

fn uuid(s: &str) -> Uuid {
    s.parse().unwrap()
}

fn fast() -> WaitOptions {
    WaitOptions::default()
        .poll_interval(Duration::from_millis(2))
        .max_poll_interval(Duration::from_millis(5))
        .max_wait(Duration::from_secs(5))
}

async fn scripted(polls: Vec<serde_json::Value>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/scraping-tasks/shopee"))
        .respond_with(Sequence::json(polls))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn fr_3_returns_one_task_per_uuid_in_input_order_once_all_are_terminal() {
    let server = scripted(vec![
        json!([task(A, "PENDING"), task(B, "RUNNING"), task(C, "BLOCKED")]),
        json!({ "items": [task(A, "RUNNING"), task(B, "SUCCESS"), task(C, "ERROR")] }),
        json!([task(A, "SUCCESS"), task(C, "BLOCKED_TOO_MANY_TIMES")]),
    ])
    .await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    let out = tasks
        .wait_for_results(&[uuid(C), uuid(A), uuid(B), uuid(A)], fast())
        .await
        .unwrap();
    let got: Vec<_> = out
        .iter()
        .map(|t| (t.uuid.unwrap(), t.status.clone()))
        .collect();
    assert_eq!(
        got,
        [
            (uuid(C), TaskStatus::BlockedTooManyTimes),
            (uuid(A), TaskStatus::Success),
            (uuid(B), TaskStatus::Success),
            (uuid(A), TaskStatus::Success)
        ]
    );
    // Only unfinished UUIDs are polled again.
    let asked: Vec<usize> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "uuids")
                .unwrap()
                .1
                .split(',')
                .count()
        })
        .collect();
    assert_eq!(asked, [3, 3, 2]);
}

#[tokio::test]
async fn fr_r1_watch_yields_each_task_once_in_completion_order() {
    let server = scripted(vec![
        json!([task(A, "RUNNING"), task(B, "SUCCESS")]),
        json!([task(A, "CANCELLED")]),
    ])
    .await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    let order: Vec<Uuid> = tasks
        .watch(&[uuid(A), uuid(B)], fast())
        .map(|t| t.unwrap().uuid.unwrap())
        .collect()
        .await;
    assert_eq!(order, [uuid(B), uuid(A)]);
}

#[tokio::test]
async fn fr_r2_dropping_the_stream_stops_polling() {
    let server = scripted(vec![json!([task(A, "SUCCESS"), task(B, "RUNNING")])]).await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    let mut stream = tasks.watch(&[uuid(A), uuid(B)], fast());
    assert_eq!(stream.next().await.unwrap().unwrap().uuid, Some(uuid(A)));
    drop(stream);
    let before = server.received_requests().await.unwrap().len();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn fr_4_times_out_with_the_latest_state_of_each_task() {
    let server = scripted(vec![json!([task(A, "SUCCESS"), task(B, "RUNNING")])]).await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    let error = tasks
        .wait_for_results(
            &[uuid(B), uuid(A)],
            fast().max_wait(Duration::from_millis(30)),
        )
        .await
        .unwrap_err();
    let Error::WaitTimeout {
        partial,
        unfinished,
        ..
    } = error
    else {
        panic!("{error:?}")
    };
    assert_eq!(unfinished, 1);
    assert_eq!(
        partial.iter().map(|t| t.status.clone()).collect::<Vec<_>>(),
        [TaskStatus::Running, TaskStatus::Success]
    );
}

#[tokio::test]
async fn missing_three_reads_in_a_row_is_task_not_found_and_a_reappearance_resets_it() {
    let server = scripted(vec![
        json!([task(A, "PENDING")]),
        json!([]),
        json!([task(A, "PENDING")]),
        json!([]),
    ])
    .await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    let error = tasks
        .wait_for_results(&[uuid(A), uuid(B)], fast())
        .await
        .unwrap_err();
    let Error::TaskNotFound { uuids, partial } = error else {
        panic!("{error:?}")
    };
    assert_eq!(uuids, [uuid(B)]);
    assert_eq!(partial.len(), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn nfr_7_unknown_statuses_keep_polling_and_foreign_tasks_are_ignored() {
    let server = scripted(vec![
        json!([task(A, "PAUSED"), task(C, "SUCCESS"), { "url": "x", "status": "SUCCESS" }]),
        json!([task(A, "SUCCESS")]),
    ])
    .await;
    let out = client(&server)
        .tasks(Scraper::Shopee)
        .wait_for_results(&[uuid(A)], fast())
        .await
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].status, TaskStatus::Success);
}

#[tokio::test]
async fn read_errors_end_the_stream_and_no_uuids_means_no_requests() {
    let server = MockServer::start().await;
    Mock::given(matchers::any())
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    let items: Vec<_> = tasks.watch(&[uuid(A)], fast()).collect().await;
    assert!(matches!(
        items.as_slice(),
        [Err(Error::Auth { status: 401, .. })]
    ));
    assert!(
        tasks
            .wait_for_results(&[], fast())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_wait_options_are_rejected_without_a_request() {
    let server = MockServer::start().await;
    let tasks = client(&server).tasks(Scraper::Shopee);
    for options in [
        fast().poll_interval(Duration::ZERO),
        fast().max_wait(Duration::ZERO),
        fast()
            .poll_interval(Duration::from_secs(10))
            .max_poll_interval(Duration::from_secs(1)),
    ] {
        assert!(matches!(
            tasks.wait_for_results(&[uuid(A)], options).await,
            Err(Error::Validation { .. })
        ));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}
