# Specification

Behaviour matches [charted-sea-client's spec](../../charted-sea-client/docs/SPEC.md): the same endpoints, defaults, status table, polling algorithm, retry policy and error categories. This document covers the Rust API shape and anything that differs.

## Public API

```rust
// Construction
Client::from_env() -> Result<Client, Error>              // CHARTED_SEA_TOKEN
Client::builder()
    .token(SecretString)                                  // or .token_from_env()
    .base_url(Url)
    .timeout(Duration)
    .retry(RetryPolicy { max_attempts, base_delay, max_delay })
    .user_agent_suffix(&str)
    .http_client(reqwest::Client)                         // bring your own pool
    .build() -> Result<Client, Error>

// Tasks: client.tasks(Scraper) -> Tasks<'_>
Tasks::run(requests) -> RunBuilder        // .wait(bool) .auto_cancel_after(Duration) .cache_max_age(Duration)
                                          // .include_fields([..]) .scraper_param(key, serde_json::Value) ; IntoFuture
Tasks::run_single(request) -> RunSingleBuilder
Tasks::read(ReadQuery) -> Result<Vec<Task>>
Tasks::pages(ReadQuery) -> impl Stream<Item = Result<Task>>
Tasks::count(CountQuery) -> Result<Vec<TaskCount>>
Tasks::cancel(CancelRequest) -> Result<CancelResponse>
Tasks::wait_for_results(&[Uuid], WaitOptions) -> Result<Vec<Task>>     // input order
Tasks::watch(&[Uuid], WaitOptions) -> impl Stream<Item = Result<Task>> // completion order

// Health and proxies
client.health().session_metrics(HealthQuery) / .task_metrics(HealthQuery)
client.proxies().validate(&[ProxyUrl]) / .read() / .update(Vec<ProxyConfig>)
ProxyUrl::parse(&str) -> Result<ProxyUrl, PlaceholderError>   // checks ${rand(..)} / ${country(..)} locally
```

## Types

- `Scraper`: `Shopee`, `ShopeeAndroid`, `Lazada`, `Other(String)`; serialises to the path names `shopee`, `shopee-android`, `lazada`.
- `TaskStatus`: the nine documented statuses plus `Unknown(String)`, `#[non_exhaustive]`; `is_terminal()` follows the shared status table ([ADR 0002](adr/0002-forward-compatible-types.md)).
- `Task`: `uuid: Option<Uuid>`, `url`, `method`, `payload: Option<serde_json::Value>`, `status`, `response_body: Option<String>`, `error_message: Option<String>`, `extra: serde_json::Map` (`#[serde(flatten)]`, unknown fields kept).
- Health enums (`HealthScraper`, `Country`, `Granularity`, `Device`, `Confidence`) follow the same pattern. Time windows: `TimeBound::Last1h | Last24h | Last72h | At(OffsetDateTime)`; ranges over 7 days are rejected before sending.

## Errors

```rust
#[non_exhaustive]
pub enum Error {
    Config(String),
    Auth { status: StatusCode },
    Validation { message: String },
    RateLimited { retry_after: Option<Duration> },
    Server { status: StatusCode },
    Network { maybe_submitted: bool, source: reqwest::Error }, // URL redacted
    Timeout,
    WaitTimeout { partial: Vec<Task> },
    TaskNotFound { uuids: Vec<Uuid> },
    Decode { source: serde_json::Error },
}
```

Task outcomes are data, never `Err` (same as [client ADR 0003](../../charted-sea-client/docs/adr/0003-error-model.md)). `Display` and `Debug` never include the token or proxy credentials.

## Differences from the TypeScript client

- `watch` returns a `Stream` in completion order; TypeScript offers `onProgress` instead.
- Cancellation is by dropping the future or stream, which is cancel-safe: no request is left half-applied on the client side. There's no `AbortSignal`.
- MSRV is declared in `Cargo.toml` (`rust-version`) and tested in CI.
