// Tests may panic on unexpected values; the no-panic rule applies to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(dead_code, missing_docs)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use charted_sea::{Client, RetryPolicy};
use serde::Deserialize;
use serde_json::Value;
use wiremock::{Match, Mock, MockServer, Request, Respond, ResponseTemplate, matchers};

pub const TOKEN: &str = "cs_test_000";

#[derive(Deserialize)]
pub struct FixtureRequest {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
}

#[derive(Deserialize)]
pub struct FixtureResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Deserialize)]
pub struct Fixture {
    pub request: FixtureRequest,
    pub response: FixtureResponse,
}

/// A documented example from `tests/fixtures/api-reference` (shared with charted-sea-client).
pub fn fixture(name: &str) -> Fixture {
    let path = format!(
        "{}/tests/fixtures/api-reference/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Matches only when the query string has exactly these parameters (no more, no fewer).
pub struct ExactQuery(pub BTreeMap<String, String>);

impl Match for ExactQuery {
    fn matches(&self, request: &Request) -> bool {
        request
            .url
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>()
            == self.0
    }
}

/// Mounts a mock that answers the fixture's request — and only that exact request — with its response.
pub async fn serve_fixture(server: &MockServer, name: &str) {
    let f = fixture(name);
    let mut mock = Mock::given(matchers::method(f.request.method.as_str()))
        .and(matchers::path(f.request.path.as_str()))
        .and(matchers::header(
            "authorization",
            format!("Bearer {TOKEN}").as_str(),
        ))
        .and(ExactQuery(f.request.query));
    if let Some(body) = f.request.body {
        mock = mock.and(matchers::body_json(body));
    }
    mock.respond_with(ResponseTemplate::new(f.response.status).set_body_json(f.response.body))
        .expect(1)
        .mount(server)
        .await;
}

/// Answers with each response in turn; the last one repeats.
pub struct Sequence {
    pub responses: Vec<ResponseTemplate>,
    pub calls: AtomicUsize,
}

impl Sequence {
    pub fn json(bodies: Vec<Value>) -> Self {
        Self::new(
            bodies
                .into_iter()
                .map(|b| ResponseTemplate::new(200).set_body_json(b))
                .collect(),
        )
    }

    pub fn new(responses: Vec<ResponseTemplate>) -> Self {
        Self {
            responses,
            calls: AtomicUsize::new(0),
        }
    }
}

impl Respond for Sequence {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses[i.min(self.responses.len() - 1)].clone()
    }
}

/// A client pointed at `server`, with millisecond retry delays.
pub fn client(server: &MockServer) -> Client {
    Client::builder()
        .token(TOKEN)
        .base_url(server.uri())
        .retry(RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(20),
        })
        .build()
        .unwrap()
}

pub fn task(uuid: &str, status: &str) -> Value {
    serde_json::json!({ "uuid": uuid, "url": format!("https://shopee.sg/i/{uuid}"), "status": status })
}
