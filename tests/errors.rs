// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Error mapping, retry policy (NFR-2, NFR-3) and secret handling (NFR-1).

mod common;

use std::time::Duration;

use charted_sea::{Client, CountQuery, Error, RetryPolicy, Scraper, TaskRequest};
use common::{Sequence, TOKEN, client};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

async fn answering(template: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(matchers::any())
        .respond_with(template)
        .mount(&server)
        .await;
    server
}

async fn count_error(server: &MockServer) -> Error {
    client(server)
        .tasks(Scraper::Shopee)
        .count(CountQuery::new())
        .await
        .unwrap_err()
}

#[tokio::test]
async fn maps_statuses_to_errors() {
    for (status, check) in [
        (
            401u16,
            (|e: &Error| matches!(e, Error::Auth { status: 401, .. })) as fn(&Error) -> bool,
        ),
        (403, |e| matches!(e, Error::Auth { status: 403, .. })),
        (400, |e| {
            matches!(
                e,
                Error::Validation {
                    status: Some(400),
                    details: Some(_),
                    ..
                }
            )
        }),
        (422, |e| {
            matches!(
                e,
                Error::Validation {
                    status: Some(422),
                    ..
                }
            )
        }),
        (404, |e| matches!(e, Error::Response { status: 404, .. })),
        (501, |e| matches!(e, Error::Server { status: 501, .. })),
    ] {
        let server =
            answering(ResponseTemplate::new(status).set_body_json(json!({ "message": "nope" })))
                .await;
        let error = count_error(&server).await;
        assert!(check(&error), "{status}: {error:?}");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "{status} is not retried"
        );
    }
    let empty = answering(ResponseTemplate::new(400)).await;
    assert!(count_error(&empty).await.to_string().ends_with("HTTP 400"));
    let wrong_shape =
        answering(ResponseTemplate::new(200).set_body_json(json!({ "unexpected": true }))).await;
    let error = count_error(&wrong_shape).await;
    assert!(
        matches!(&error, Error::Response { status: 200, message, .. } if message.contains("documented shape")),
        "{error:?}"
    );
    let html = answering(ResponseTemplate::new(200).set_body_string("<html>")).await;
    assert!(matches!(
        count_error(&html).await,
        Error::Response { status: 200, .. }
    ));
}

#[tokio::test]
async fn nfr_3_retries_idempotent_calls_on_transient_failures_up_to_max_attempts() {
    let server = MockServer::start().await;
    Mock::given(matchers::any())
        .respond_with(Sequence::new(vec![
            ResponseTemplate::new(503),
            ResponseTemplate::new(408),
            ResponseTemplate::new(200).set_body_json(json!([])),
        ]))
        .mount(&server)
        .await;
    assert!(
        client(&server)
            .tasks(Scraper::Shopee)
            .count(CountQuery::new())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 3);

    let always = answering(ResponseTemplate::new(502)).await;
    assert!(matches!(
        count_error(&always).await,
        Error::Server { status: 502, .. }
    ));
    assert_eq!(always.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn nfr_3_honours_retry_after_but_gives_up_when_it_exceeds_max_delay() {
    let server = MockServer::start().await;
    Mock::given(matchers::any())
        .respond_with(Sequence::new(vec![
            ResponseTemplate::new(429).insert_header("retry-after", "0"),
            ResponseTemplate::new(200).set_body_json(json!([])),
        ]))
        .mount(&server)
        .await;
    assert!(
        client(&server)
            .tasks(Scraper::Shopee)
            .count(CountQuery::new())
            .await
            .is_ok()
    );

    let slow = answering(ResponseTemplate::new(429).insert_header("retry-after", "60")).await;
    let error = count_error(&slow).await;
    assert!(
        matches!(error, Error::RateLimited { retry_after: Some(d), .. } if d == Duration::from_secs(60)),
        "{error:?}"
    );
    assert_eq!(slow.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn nfr_2_never_retries_run_and_flags_it_maybe_submitted() {
    let server = answering(ResponseTemplate::new(200).set_delay(Duration::from_secs(5))).await;
    let client = Client::builder()
        .token(TOKEN)
        .base_url(server.uri())
        .timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    let error = client
        .tasks(Scraper::Shopee)
        .run([TaskRequest::get("u")])
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Timeout {
                maybe_submitted: true,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    let busy = answering(ResponseTemplate::new(503)).await;
    let error = common::client(&busy)
        .tasks(Scraper::Shopee)
        .run([TaskRequest::get("u")])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Server { status: 503, .. }));
    assert_eq!(busy.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn connection_failures_are_network_errors() {
    let client = Client::builder()
        .token(TOKEN)
        .base_url("http://127.0.0.1:9")
        .retry(RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        })
        .build()
        .unwrap();
    let error = client
        .tasks(Scraper::Shopee)
        .count(CountQuery::new())
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Network {
                maybe_submitted: false,
                ..
            }
        ),
        "{error:?}"
    );
    let run = client
        .tasks(Scraper::Shopee)
        .run([TaskRequest::get("u")])
        .await
        .unwrap_err();
    assert!(matches!(
        run,
        Error::Network {
            maybe_submitted: true,
            ..
        }
    ));
    assert!(std::error::Error::source(&run).is_some());
}

#[tokio::test]
async fn nfr_1_the_token_never_appears_in_errors_or_debug_output() {
    let secret = "cs_sentinel_9f8e7d6c5b4a";
    for status in [400u16, 401, 404, 429, 500] {
        let server = answering(ResponseTemplate::new(status).set_body_json(
            json!({ "error": format!("bad token {secret}"), "auth": format!("Bearer {secret}") }),
        ))
        .await;
        let client = Client::builder()
            .token(secret)
            .base_url(server.uri())
            .retry(RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            })
            .build()
            .unwrap();
        let error = client
            .tasks(Scraper::Shopee)
            .count(CountQuery::new())
            .await
            .unwrap_err();
        for text in [
            error.to_string(),
            format!("{error:?}"),
            format!("{client:?}"),
        ] {
            assert!(!text.contains(secret), "{status}: {text}");
        }
    }
    let builder = Client::builder().token(secret);
    assert!(!format!("{builder:?}").contains(secret));
}

#[test]
fn builder_validates_options() {
    let bad = [
        Client::builder().build(),
        Client::builder().token("  ").build(),
        Client::builder().token("t").base_url("not a url").build(),
        Client::builder().token("t").base_url("ftp://x").build(),
        Client::builder().token("t").timeout(Duration::ZERO).build(),
        Client::builder()
            .token("t")
            .retry(RetryPolicy {
                max_attempts: 0,
                ..RetryPolicy::default()
            })
            .build(),
        Client::builder()
            .token("t")
            .retry(RetryPolicy {
                max_attempts: 1,
                base_delay: Duration::from_secs(5),
                max_delay: Duration::from_secs(1),
            })
            .build(),
    ];
    assert!(bad.into_iter().all(|r| matches!(r, Err(Error::Config(_)))));
    let client = Client::builder()
        .token("t")
        .base_url("http://localhost:8080/api/")
        .user_agent_suffix(" my-app/2 ")
        .http_client(reqwest::Client::new())
        .build()
        .unwrap();
    assert!(format!("{client:?}").contains("http://localhost:8080/api\""));
}

#[tokio::test]
async fn sends_auth_and_user_agent_and_reads_the_env_token() {
    let server = MockServer::start().await;
    Mock::given(matchers::header(
        "authorization",
        format!("Bearer {TOKEN}").as_str(),
    ))
    .and(matchers::header(
        "user-agent",
        format!("charted-sea-rs/{} my-app/2", env!("CARGO_PKG_VERSION")).as_str(),
    ))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
    .expect(1)
    .mount(&server)
    .await;
    let client = Client::builder()
        .token(TOKEN)
        .base_url(server.uri())
        .user_agent_suffix("my-app/2")
        .build()
        .unwrap();
    client
        .tasks(Scraper::Shopee)
        .count(CountQuery::new())
        .await
        .unwrap();
    // from_env reads CHARTED_SEA_TOKEN: a client when it's set, a Config error when it isn't.
    match std::env::var("CHARTED_SEA_TOKEN") {
        Ok(_) => assert!(Client::from_env().is_ok()),
        Err(_) => assert!(matches!(Client::from_env(), Err(Error::Config(_)))),
    }
}
