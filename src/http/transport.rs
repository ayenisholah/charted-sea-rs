//! One request: auth, timeout, JSON, status → error mapping, retries for idempotent calls, redaction.

use std::time::{Duration, SystemTime};

use reqwest::header::{ACCEPT, RETRY_AFTER, USER_AGENT};
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::redact::redact;
use super::retry::{backoff_delay, is_retryable_status, parse_retry_after};
use crate::client::Inner;
use crate::error::{Error, Result};

const MAX_MESSAGE_CHARS: usize = 300;

/// A request to the Charted Sea API.
pub(crate) struct ApiRequest {
    pub method: reqwest::Method,
    pub path: String,
    pub query: Vec<(&'static str, String)>,
    pub body: Option<Value>,
    /// Only idempotent requests are retried (ADR 0004 of the TypeScript client).
    pub idempotent: bool,
    pub timeout: Option<Duration>,
}

impl ApiRequest {
    pub(crate) fn get(path: String, query: Vec<(&'static str, String)>) -> Self {
        Self {
            method: reqwest::Method::GET,
            path,
            query,
            body: None,
            idempotent: true,
            timeout: None,
        }
    }

    fn endpoint(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

fn retryable(error: &Error) -> bool {
    match error {
        Error::Network { .. } | Error::Timeout { .. } | Error::RateLimited { .. } => true,
        Error::Server { status, .. } | Error::Response { status, .. } => {
            is_retryable_status(*status)
        }
        _ => false,
    }
}

impl Inner {
    fn clean(&self, text: &str) -> String {
        redact(text, &[self.token.expose_secret()])
    }

    /// Sends `req`, retrying idempotent requests on transient failures, and decodes the JSON body.
    pub(crate) async fn send<T: DeserializeOwned>(&self, req: &ApiRequest) -> Result<T> {
        let (status, json) = self.send_json(req).await?;
        serde_json::from_value(json).map_err(|e| Error::Response {
            endpoint: req.endpoint(),
            status,
            message: self.clean(&format!("body is not the documented shape ({e})")),
        })
    }

    /// The retry loop, kept non-generic so it is compiled (and measured) once.
    async fn send_json(&self, req: &ApiRequest) -> Result<(u16, Value)> {
        let max_attempts = if req.idempotent {
            self.retry.max_attempts
        } else {
            1
        };
        let mut attempt = 1;
        loop {
            let error = match self.attempt(req).await {
                Ok(value) => return Ok(value),
                Err(error) => error,
            };
            if attempt >= max_attempts || !retryable(&error) {
                return Err(error);
            }
            let retry_after = match &error {
                Error::RateLimited { retry_after, .. } => *retry_after,
                _ => None,
            };
            // Honour Retry-After, but never wait longer than max_delay: give up instead.
            if retry_after.is_some_and(|ra| ra > self.retry.max_delay) {
                return Err(error);
            }
            let delay =
                retry_after.unwrap_or_else(|| backoff_delay(attempt, &self.retry, self.jitter()));
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    async fn attempt(&self, req: &ApiRequest) -> Result<(u16, Value)> {
        let (status, retry_after, text) = self
            .exchange(req)
            .await
            .map_err(|e| self.transport_error(e, req))?;
        let endpoint = req.endpoint();
        if !(200..300).contains(&status) {
            return Err(self.status_error(endpoint, status, &text, retry_after));
        }
        let json = serde_json::from_str(&text).map_err(|e| Error::Response {
            endpoint,
            status,
            message: self.clean(&format!("body is not JSON ({e})")),
        })?;
        Ok((status, json))
    }

    /// Sends the request and reads the whole body. Any transport failure (connect, timeout,
    /// body read, or a malformed URL that reqwest rejects) comes back as a `reqwest::Error`.
    async fn exchange(&self, req: &ApiRequest) -> reqwest::Result<(u16, Option<Duration>, String)> {
        let mut builder = self
            .http
            .request(req.method.clone(), format!("{}{}", self.base_url, req.path))
            .query(&req.query)
            .bearer_auth(self.token.expose_secret())
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, &self.user_agent)
            .timeout(req.timeout.unwrap_or(self.timeout));
        if let Some(body) = &req.body {
            builder = builder.json(body);
        }
        let response = builder.send().await?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, SystemTime::now()));
        Ok((status, retry_after, response.text().await?))
    }

    fn transport_error(&self, error: reqwest::Error, req: &ApiRequest) -> Error {
        let endpoint = req.endpoint();
        let maybe_submitted = req.method != reqwest::Method::GET && !req.idempotent;
        if error.is_timeout() {
            Error::Timeout {
                endpoint,
                maybe_submitted,
            }
        } else {
            let source = self.clean(&error.without_url().to_string());
            Error::Network {
                endpoint,
                maybe_submitted,
                source: source.into(),
            }
        }
    }

    fn status_error(
        &self,
        endpoint: String,
        status: u16,
        body: &str,
        retry_after: Option<Duration>,
    ) -> Error {
        let clean = self.clean(body);
        let message: String = clean.chars().take(MAX_MESSAGE_CHARS).collect();
        match status {
            401 | 403 => Error::Auth { endpoint, status },
            429 => Error::RateLimited {
                endpoint,
                retry_after,
            },
            400 | 422 => Error::Validation {
                endpoint,
                status: Some(status),
                message: if message.is_empty() {
                    format!("HTTP {status}")
                } else {
                    message
                },
                details: serde_json::from_str(&clean).ok(),
            },
            500..=599 => Error::Server {
                endpoint,
                status,
                message,
            },
            _ => Error::Response {
                endpoint,
                status,
                message: format!("HTTP {status}: {message}"),
            },
        }
    }
}
