//! Errors. Faults are `Err`; task outcomes (blocked, failed, cancelled) are data in [`crate::Task`].

use std::time::Duration;

use crate::Task;

/// Every fault the client returns. `Display` and `Debug` never contain the token or proxy credentials.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Missing token or an invalid option.
    #[error("configuration error: {0}")]
    Config(String),
    /// The token was rejected (HTTP 401 or 403).
    #[error("{endpoint} was rejected (HTTP {status}); check the API token")]
    Auth {
        /// e.g. `GET /scraping-tasks/shopee`
        endpoint: String,
        /// 401 or 403
        status: u16,
    },
    /// The request was rejected (HTTP 400/422), or a local check failed before sending.
    #[error("{endpoint} rejected the request: {message}")]
    Validation {
        /// Endpoint, or `client` for local checks.
        endpoint: String,
        /// HTTP status, when the server answered.
        status: Option<u16>,
        /// Redacted message.
        message: String,
        /// Parsed error body, redacted.
        details: Option<serde_json::Value>,
    },
    /// HTTP 429.
    #[error("{endpoint} was rate limited (HTTP 429)")]
    RateLimited {
        /// Endpoint.
        endpoint: String,
        /// From `Retry-After`, when present.
        retry_after: Option<Duration>,
    },
    /// HTTP 5xx.
    #[error("{endpoint} failed with HTTP {status}: {message}")]
    Server {
        /// Endpoint.
        endpoint: String,
        /// 5xx status.
        status: u16,
        /// Redacted start of the body.
        message: String,
    },
    /// Any other unexpected status, or a body that isn't the documented JSON.
    #[error("{endpoint} returned an unexpected response: {message}")]
    Response {
        /// Endpoint.
        endpoint: String,
        /// HTTP status.
        status: u16,
        /// What was wrong.
        message: String,
    },
    /// The connection failed. For `run`, `run_single`, `cancel` and proxy `update`, `maybe_submitted`
    /// is true: re-run with a cache age to get results without paying twice.
    #[error("{endpoint} failed: network error")]
    Network {
        /// Endpoint.
        endpoint: String,
        /// The server may have acted on the request.
        maybe_submitted: bool,
        /// Underlying error, URL removed.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The request exceeded its timeout.
    #[error("{endpoint} timed out")]
    Timeout {
        /// Endpoint.
        endpoint: String,
        /// The server may have acted on the request.
        maybe_submitted: bool,
    },
    /// `wait_for_results` / `watch` reached `max_wait`.
    #[error("gave up waiting after {waited:?} with {unfinished} task(s) unfinished")]
    WaitTimeout {
        /// How long it waited.
        waited: Duration,
        /// Tasks not yet terminal.
        unfinished: usize,
        /// Latest known state of every task, in input order.
        partial: Vec<Task>,
    },
    /// Some UUIDs were missing from 3 consecutive reads.
    #[error("{} task(s) not found after 3 reads", uuids.len())]
    TaskNotFound {
        /// The missing UUIDs.
        uuids: Vec<uuid::Uuid>,
        /// Latest known state of the others, in input order.
        partial: Vec<Task>,
    },
}

impl Error {
    pub(crate) fn local(message: impl Into<String>) -> Self {
        Self::Validation {
            endpoint: "client".into(),
            status: None,
            message: message.into(),
            details: None,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
