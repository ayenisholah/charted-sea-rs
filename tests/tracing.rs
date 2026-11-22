// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The `tracing` feature records endpoint, status and duration, never the token.
#![cfg(feature = "tracing")]

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use charted_sea::{CountQuery, Scraper};
use common::{TOKEN, client};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn records_endpoint_and_status_but_never_the_token() {
    let server = MockServer::start().await;
    Mock::given(matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    client(&server)
        .tasks(Scraper::Shopee)
        .count(CountQuery::new())
        .await
        .unwrap();
    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("endpoint=GET /scraping-tasks/shopee/count"),
        "{logs}"
    );
    assert!(logs.contains("status=200"), "{logs}");
    assert!(!logs.contains(TOKEN));
}
