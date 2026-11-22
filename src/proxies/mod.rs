//! Proxy configuration: validate proxy URLs against each scraper, read and replace the
//! account's configuration, and check URL templates locally ([`ProxyUrl::parse`]).
//!
//! Proxy URLs carry provider credentials, so `Debug` on these types hides the userinfo.

mod placeholder;

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use placeholder::{Placeholder, PlaceholderError, PlaceholderKind, PlaceholderValue, ProxyUrl};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::http::redact::redact;
use crate::http::transport::ApiRequest;

/// One protocol/port check from Validate Proxies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ProxyTest {
    /// `http`, `https`, `http3`, `stun`, `tcp20443` or `udp20443`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Whether the check passed.
    pub success: bool,
    /// The IP the remote server saw, when it passed.
    #[serde(default)]
    pub ip_address: Option<String>,
    /// Warnings, e.g. a different IP from the HTTP check.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Errors, when it failed.
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Whether a proxy works with one scraper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ProxyCompatibility {
    /// `SHOPEE`, `SHOPEE_ANDROID`, `LAZADA`, …
    pub scraper: String,
    /// Human-readable scraper name.
    #[serde(default)]
    pub description: String,
    /// Fully compatible.
    pub compatible: bool,
    /// Why not, when incompatible.
    #[serde(default)]
    pub incompatibility_reason: Option<String>,
    /// Why performance may be lower, even when compatible.
    #[serde(default)]
    pub degraded_performance_reason: Option<String>,
}

/// Validation result for one proxy URL.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProxyValidation {
    /// The URL as sent (credentials included; `Debug` hides them).
    pub url: String,
    /// Protocol/port checks.
    #[serde(default)]
    pub tests: Vec<ProxyTest>,
    /// Compatibility per scraper.
    #[serde(default)]
    pub compatibilities: Vec<ProxyCompatibility>,
}

impl fmt::Debug for ProxyValidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyValidation")
            .field("url", &redact(&self.url, &[]))
            .field("tests", &self.tests)
            .field("compatibilities", &self.compatibilities)
            .finish()
    }
}

/// A set of proxy URLs of one type, optionally for one country.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProxySetting {
    /// `DATACENTER` or `RESIDENTIAL`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Proxy URLs; `${rand(...)}` and `${country(...)}` are expanded server-side.
    pub urls: Vec<String>,
    /// When set, this setting is dedicated to that country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Every other field (e.g. `invalidUrls` in an error echo).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ProxySetting {
    /// A setting of `kind` (`DATACENTER` or `RESIDENTIAL`) with these URLs.
    pub fn new<S: Into<String>>(
        kind: impl Into<String>,
        urls: impl IntoIterator<Item = S>,
    ) -> Self {
        Self {
            kind: kind.into(),
            urls: urls.into_iter().map(Into::into).collect(),
            country: None,
            extra: Map::new(),
        }
    }

    /// Dedicate this setting to one country.
    #[must_use]
    pub fn country(mut self, country: impl Into<String>) -> Self {
        self.country = Some(country.into());
        self
    }
}

impl fmt::Debug for ProxySetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let urls: Vec<String> = self.urls.iter().map(|u| redact(u, &[])).collect();
        f.debug_struct("ProxySetting")
            .field("kind", &self.kind)
            .field("urls", &urls)
            .field("country", &self.country)
            .finish_non_exhaustive()
    }
}

/// Proxy configuration for one scraper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ProxyConfig {
    /// `SHOPEE`, `SHOPEE_ANDROID` or `LAZADA`.
    pub scraper: String,
    /// The settings.
    pub proxy_settings: Vec<ProxySetting>,
}

impl ProxyConfig {
    /// Configuration for `scraper` (upper-case name, e.g. `SHOPEE`).
    pub fn new(
        scraper: impl Into<String>,
        settings: impl IntoIterator<Item = ProxySetting>,
    ) -> Self {
        Self {
            scraper: scraper.into(),
            proxy_settings: settings.into_iter().collect(),
        }
    }
}

/// The proxies API. Get one with [`Client::proxies`].
#[derive(Debug, Clone)]
pub struct Proxies {
    client: Client,
}

impl Proxies {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Checks proxy URLs against each scraper without changing the configuration.
    /// Consumes a little proxy traffic.
    pub async fn validate<S: AsRef<str>>(&self, urls: &[S]) -> Result<Vec<ProxyValidation>> {
        self.validate_strings(urls.iter().map(|u| u.as_ref().to_owned()).collect())
            .await
    }

    async fn validate_strings(&self, urls: Vec<String>) -> Result<Vec<ProxyValidation>> {
        if urls.is_empty() {
            return Err(Error::local("validate needs at least one proxy URL"));
        }
        let req = ApiRequest {
            method: reqwest::Method::POST,
            path: "/proxy-validate".into(),
            query: Vec::new(),
            body: Some(Value::from(urls)),
            idempotent: true,
            timeout: None,
        };
        self.client.inner.send(&req).await
    }

    /// Reads the account's proxy configuration.
    pub async fn read(&self) -> Result<Vec<ProxyConfig>> {
        self.client
            .inner
            .send(&ApiRequest::get("/proxy-configs".into(), Vec::new()))
            .await
    }

    /// Replaces the **whole** proxy configuration. All-or-nothing on the server: if any URL is
    /// invalid, nothing changes and [`Error::Validation`] carries the echoed request with
    /// `invalidUrls` per setting in `details` (credentials redacted). Never retried.
    pub async fn update(&self, configs: &[ProxyConfig]) -> Result<Vec<ProxyConfig>> {
        let req = ApiRequest {
            method: reqwest::Method::PUT,
            path: "/proxy-configs".into(),
            query: Vec::new(),
            // Plain structs with string keys always serialize.
            body: Some(serde_json::to_value(configs).unwrap_or_default()),
            idempotent: false,
            timeout: None,
        };
        self.client.inner.send(&req).await
    }
}
