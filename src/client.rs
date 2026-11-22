//! [`Client`] and [`ClientBuilder`].

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};

use crate::error::{Error, Result};
use crate::health::Health;
use crate::http::retry::{Jitter, RetryPolicy};
use crate::proxies::Proxies;
use crate::tasks::Tasks;
use crate::types::Scraper;

/// Default API host.
pub const DEFAULT_BASE_URL: &str = "https://continuous-scraper.common.chartedapi.com";

pub(crate) struct Inner {
    pub http: reqwest::Client,
    /// Without a trailing slash.
    pub base_url: String,
    pub token: SecretString,
    pub timeout: Duration,
    pub retry: RetryPolicy,
    pub user_agent: String,
    jitter: Mutex<Jitter>,
}

impl Inner {
    /// Uniform in `[0, 1)`; 0.5 if the lock was poisoned.
    pub(crate) fn jitter(&self) -> f64 {
        self.jitter.lock().map_or(0.5, |mut j| j.next_f64())
    }
}

/// A Charted Sea API client. Cheap to clone; clones share one connection pool.
///
/// `Debug` never prints the token.
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
}

impl Client {
    /// Builds a client with the token from `CHARTED_SEA_TOKEN` and default settings.
    pub fn from_env() -> Result<Self> {
        Self::builder().token_from_env().build()
    }

    /// Starts a [`ClientBuilder`].
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The tasks API for one scraper, e.g. `client.tasks(Scraper::Shopee)` or `client.tasks("lazada")`.
    pub fn tasks(&self, scraper: impl Into<Scraper>) -> Tasks {
        Tasks::new(self.clone(), scraper.into())
    }

    /// The health-metrics API.
    #[must_use]
    pub fn health(&self) -> Health {
        Health::new(self.clone())
    }

    /// The proxy-configuration API.
    #[must_use]
    pub fn proxies(&self) -> Proxies {
        Proxies::new(self.clone())
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.inner.base_url)
            .field("token", &"[REDACTED]")
            .field("timeout", &self.inner.timeout)
            .field("retry", &self.inner.retry)
            .finish_non_exhaustive()
    }
}

/// Configures a [`Client`].
#[derive(Default)]
pub struct ClientBuilder {
    token: Option<SecretString>,
    base_url: Option<String>,
    timeout: Option<Duration>,
    retry: Option<RetryPolicy>,
    user_agent_suffix: Option<String>,
    http: Option<reqwest::Client>,
}

impl ClientBuilder {
    /// Sets the API token.
    #[must_use]
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(SecretString::from(token.into()));
        self
    }

    /// Reads the API token from `CHARTED_SEA_TOKEN`.
    #[must_use]
    pub fn token_from_env(mut self) -> Self {
        self.token = std::env::var("CHARTED_SEA_TOKEN")
            .ok()
            .map(SecretString::from);
        self
    }

    /// Overrides the API base URL (default [`DEFAULT_BASE_URL`]).
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Per-request timeout (default 30 s). Synchronous runs use their auto-cancel time + 30 s.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Retry policy for idempotent calls (default 3 attempts, 500 ms base, 10 s cap).
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Appended to the `charted-sea-rs/<version>` user agent.
    #[must_use]
    pub fn user_agent_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.user_agent_suffix = Some(suffix.into());
        self
    }

    /// Uses your own `reqwest::Client` (and its connection pool, proxy and TLS settings).
    #[must_use]
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// Validates the options and builds the client.
    pub fn build(self) -> Result<Client> {
        let token = self
            .token
            .filter(|t| !t.expose_secret().trim().is_empty())
            .ok_or_else(|| {
                Error::Config("no API token; set CHARTED_SEA_TOKEN or call .token(...)".into())
            })?;
        let base = self.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let parsed = url::Url::parse(&base)
            .map_err(|_| Error::Config(format!("base_url is not a valid URL: {base}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(Error::Config(format!(
                "base_url must be http(s), got {}",
                parsed.scheme()
            )));
        }
        let timeout = self.timeout.unwrap_or(Duration::from_secs(30));
        if timeout.is_zero() {
            return Err(Error::Config("timeout must be greater than zero".into()));
        }
        let retry = self.retry.unwrap_or_default();
        if retry.max_attempts == 0 || retry.base_delay > retry.max_delay {
            return Err(Error::Config(
                "retry needs max_attempts ≥ 1 and base_delay ≤ max_delay".into(),
            ));
        }
        let http = match self.http {
            Some(http) => http,
            None => reqwest::Client::builder()
                .build()
                .map_err(http_client_error)?,
        };
        let mut user_agent = format!("charted-sea-rs/{}", env!("CARGO_PKG_VERSION"));
        if let Some(suffix) = self.user_agent_suffix.filter(|s| !s.trim().is_empty()) {
            user_agent = format!("{user_agent} {}", suffix.trim());
        }
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base_url: parsed.as_str().trim_end_matches('/').to_owned(),
                token,
                timeout,
                retry,
                user_agent,
                jitter: Mutex::new(Jitter::seeded()),
            }),
        })
    }
}

fn http_client_error(e: reqwest::Error) -> Error {
    Error::Config(format!(
        "couldn't build the HTTP client: {}",
        e.without_url()
    ))
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn http_client_build_failures_become_config_errors() {
        // An invalid header name is the simplest way to make reqwest return an error.
        let error = reqwest::Client::new()
            .get("http://x")
            .header("bad header", "v")
            .build()
            .unwrap_err();
        assert!(
            matches!(http_client_error(error), Error::Config(m) if m.starts_with("couldn't build the HTTP client"))
        );
    }
}
