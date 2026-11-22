# Specification

Behaviour matches [charted-sea-client's spec](https://github.com/ayenisholah/charted-sea-client/blob/main/docs/SPEC.md): the same endpoints, defaults, status table, polling algorithm, retry policy and error categories ([ADR 0004](adr/0004-shared-semantics-with-typescript.md)). This document covers the Rust API shape and anything that differs.

Source of truth for behaviour: the [Charted Sea API reference](https://chartedsea.com/docs/api-reference) plus what the live API was seen to do on 2026-11-22 (listed under [Live API findings](#live-api-findings)).

## Public API (built)

```rust
// Construction
Client::from_env() -> Result<Client>                       // CHARTED_SEA_TOKEN
Client::builder()
    .token(impl Into<String>)                               // stored as SecretString; or .token_from_env()
    .base_url(impl Into<String>)                            // default DEFAULT_BASE_URL
    .timeout(Duration)                                      // default 30 s
    .retry(RetryPolicy { max_attempts, base_delay, max_delay }) // default 3 / 500 ms / 10 s
    .user_agent_suffix(impl Into<String>)                   // appended to "charted-sea-rs/<version>"
    .http_client(reqwest::Client)                           // bring your own pool, proxy, TLS
    .build() -> Result<Client>

// Tasks: client.tasks(impl Into<Scraper>) -> Tasks   (owned; Client is an Arc inside)
Tasks::run(impl IntoIterator<Item = TaskRequest>) -> Run<Vec<Task>>
Tasks::run_single(TaskRequest) -> Run<Task>
    // Run<T>: .wait(bool) .auto_cancel_after(Duration) .cache_max_age(Duration)
    //         .include_all_fields() .include_fields([..]) .scraper_param(key, serde_json::Value)
    //         then `.await` (IntoFuture) → Result<T>
Tasks::read(ReadQuery) -> Result<Vec<Task>>
    // ReadQuery::new().uuids(..).urls(..).limit(n).offset(n).min_pending_at(t).max_pending_at(t)
    //                 .include_all_fields().include_fields(..)
Tasks::count(CountQuery) -> Result<Vec<TaskCount>>          // CountQuery::new().statuses(..).min_pending_at(t)
Tasks::cancel(CancelRequest) -> Result<CancelResponse>      // CancelRequest::new().uuids(..).countries(..)
Tasks::wait_for_results(&[Uuid], WaitOptions) -> Result<Vec<Task>>  // input order
Tasks::watch(&[Uuid], WaitOptions) -> BoxStream<'static, Result<Task>> // completion order
    // WaitOptions::default().poll_interval(d).max_poll_interval(d).max_wait(d)

redact(&str, &[&str]) -> String                            // the redaction the crate uses, for your own logs
```

Re-exported for convenience: `Uuid` (uuid), `OffsetDateTime` (time).

### Planned (M2)

```rust
Tasks::pages(ReadQuery) -> impl Stream<Item = Result<Task>>          // offset paging, like tasks.iterate in TypeScript
client.health().session_metrics(HealthQuery) / .task_metrics(HealthQuery)
client.proxies().validate(&[ProxyUrl]) / .read() / .update(Vec<ProxyConfig>)
ProxyUrl::parse(&str) -> Result<ProxyUrl, PlaceholderError>         // ${rand(..)} / ${country(..)} checked locally
```

Plus a `tracing` feature (spans per request with endpoint and status, never headers or bodies).

## Types

- `Scraper`: `Shopee`, `ShopeeAndroid`, `Lazada`, `Other(String)`, `#[non_exhaustive]`. `as_str()`/`Display` give the path names `shopee`, `shopee-android`, `lazada`. `From<&str>`.
- `TaskStatus`: the nine documented statuses plus `Unknown(String)`, `#[non_exhaustive]`. Serialises as the API's upper-case names. `is_terminal()` follows the shared status table: `Success`, `BlockedTooManyTimes`, `ErrorTooManyTimes`, `Cancelled` and `Replaced` are terminal. `Blocked`/`Error` (retried by the server) and unknown statuses are not ([ADR 0002](adr/0002-forward-compatible-types.md)).
- `TaskRequest`: `TaskRequest::get(url)` or `TaskRequest::post(url, payload)`.
- `Task` (`#[non_exhaustive]`): `uuid: Option<Uuid>`, `url`, `method: Option<String>`, `payload: Option<Value>`, `status`, `response_body: Option<String>`, `error_message: Option<String>`, and `extra: Map<String, Value>` (`#[serde(flatten)]`). Unknown fields are kept, e.g. the live API's `requestBody`.
- `TaskCount { country, status, count }` and `CancelResponse { uuids, countries }`, both `#[non_exhaustive]`.

## Errors

```rust
#[non_exhaustive]
pub enum Error {
    Config(String),                                                   // builder / missing token
    Auth { endpoint, status },                                        // 401, 403
    Validation { endpoint, status: Option<u16>, message, details },   // 400, 422, or a local check ("client")
    RateLimited { endpoint, retry_after: Option<Duration> },          // 429
    Server { endpoint, status, message },                             // 5xx
    Response { endpoint, status, message },                           // other status, non-JSON or wrong-shape body
    Network { endpoint, maybe_submitted, source },                    // connect/read failure; URL stripped
    Timeout { endpoint, maybe_submitted },
    WaitTimeout { waited, unfinished, partial: Vec<Task> },
    TaskNotFound { uuids: Vec<Uuid>, partial: Vec<Task> },
}
```

Task outcomes are data, never `Err` (same as [the client's ADR 0003](https://github.com/ayenisholah/charted-sea-client/blob/main/docs/adr/0003-error-model.md)). `Display` and `Debug` never include the token or proxy credentials. Messages and `details` pass through `redact` first.

`maybe_submitted` is true for `run`, `run_single` and `cancel`: the server may have acted on the request. Status codes are `u16`, so no `reqwest` types appear in the public error API. `Network::source` is a boxed error.

## Behaviour decided while building

- **Retries:** idempotent calls only (reads, counts). Retried statuses are 408, 425, 429, 500, 502, 503 and 504, plus network errors and timeouts, with full-jitter exponential backoff. A `Retry-After` longer than `max_delay` is not waited out: `RateLimited` is returned instead.
- **Local checks** return `Error::Validation { endpoint: "client", .. }` before any request: an empty `run`, a `scraper_param` that would overwrite `requests`/`cacheMaxAgeInSec`/`url`/`method`/`payload`, a `cancel` with neither UUIDs nor countries (so a mistake can't cancel everything), `limit`/`offset` with more than 100 UUIDs, an empty scraper name, and invalid `WaitOptions`.
- **Reads by UUID** set `limit` to the batch size unless you set one; otherwise the API's default of 20 would silently drop tasks. Duplicate UUIDs are requested once.
- **Synchronous runs** (`.wait(true)`) use `auto_cancel_after` (default 600 s) + 30 s as the HTTP timeout.
- **Polling** reads only unfinished UUIDs, resets the backoff when any task finishes, sleeps at most until the deadline (so there's one last poll at `max_wait`), and stops at the first error.

## Live API findings

- Async `run` returns the task array with `uuid`, `PENDING` and `requestBody: null`. This answers open question 1.
- **Read returns `{ items, total, pageRequest }`**, not the documented bare array. Both shapes are accepted (`tests/fixtures/api-reference/read-tasks-live.json`).
- An unknown scraper returns 200 with an empty result, not 404.

## Differences from the TypeScript client

- `watch` returns a stream in completion order; TypeScript offers an `onProgress` callback instead.
- Cancellation is by dropping the future or stream. Polling stops immediately and no request is left half-applied. There's no `AbortSignal`.
- Builders (`Run`, `ReadQuery`, `CountQuery`, `CancelRequest`, `WaitOptions`) replace option objects.
- Timestamps are `time::OffsetDateTime`, sent as RFC 3339.
- MSRV is **1.89**, declared in `Cargo.toml` (`rust-version`) and checked in CI. The `uuid` dependency sets it.
