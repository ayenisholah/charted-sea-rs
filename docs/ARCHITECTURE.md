# Architecture

Built: the tasks API (M0–M1). Health, proxies and `pages` are planned (M2).

```
src/
  lib.rs           re-exports and crate docs (quick start as a doctest)
  client.rs        Client (Arc<Inner>), ClientBuilder, hand-written redacting Debug
  error.rs         Error enum and Result alias
  types.rs         Scraper, TaskStatus, TaskRequest, Task, TaskCount, CancelResponse, TaskPage (pure)
  http/
    transport.rs   one request: auth, timeout, JSON, status → Error, retries for idempotent calls
    retry.rs       RetryPolicy, full-jitter backoff, poll delay, Retry-After, Jitter PRNG (pure)
    redact.rs      token, Bearer and URL-userinfo redaction (pure)
  tasks/
    mod.rs         Tasks: run, run_single (Run<T> builder + IntoFuture), read, count, cancel;
                   ReadQuery, CountQuery, CancelRequest
    wait.rs        watch (stream) and wait_for_results (collects watch, restores input order)
    batch.rs       UUID de-duplication and order-preserving merge (pure)
tests/
  common/mod.rs    fixture loader, exact-query matcher, scripted responder, test client
  contract.rs      every endpoint against the documented examples (FR-1, FR-2, FR-5, FR-9)
  polling.rs       watch / wait_for_results (FR-3, FR-4, FR-R1, FR-R2, NFR-7)
  errors.rs        status mapping, retries, secrets (NFR-1 to NFR-3), builder validation
  quickstart.rs    the README quick start against a mock server
  live.rs          #[ignore]d live smoke test
  fixtures/        shared with charted-sea-client (hash-checked in CI)
```

## Design rules

- **One connection pool.** `Client` is `Clone + Send + Sync` (an `Arc` inside) and reuses one `reqwest::Client`. `Tasks` holds a clone, so streams are `'static`.
- **Secrets.** The token is a `secrecy::SecretString`, exposed only when the `Authorization` header is built ([ADR 0003](adr/0003-secret-handling.md)). Every server message and every network error passes through `redact` before it goes into an `Error`. `reqwest` errors are stripped of their URL.
- **Safety.** `unsafe_code = "forbid"`, `missing_docs = "deny"`, and Clippy pedantic with `unwrap_used`, `expect_used` and `panic` denied. Test files allow panics, which are how tests fail.
- **Forward compatibility.** API enums have `Unknown`/`Other` variants and are `#[non_exhaustive]`, and response structs keep unknown fields ([ADR 0002](adr/0002-forward-compatible-types.md)).
- **Generic code stays thin.** The retry loop (`send_json`), the run request builder (`RunSpec`) and `run_vec` are non-generic. Generic wrappers only convert types. This keeps code size down, and every line is measured once by coverage.

## Request flow

1. `Run<T>` (or `read`, `count`, `cancel`) builds an `ApiRequest`: method, path, query, body, whether it's idempotent, and the timeout.
2. `Inner::send_json` runs `attempt` up to `max_attempts` times, retrying only idempotent requests, and only on network or timeout errors, 408, 425, 429 or 5xx (except 501).
3. `attempt` → `exchange` sends with reqwest and reads the body. Any transport failure becomes `Network` or `Timeout` with `maybe_submitted`. A non-2xx status is mapped by `status_error`.
4. `send::<T>` decodes the JSON into `T`. A wrong shape is `Error::Response`.

## Polling as a stream

`watch` is `stream::unfold` over a `State`: `{ pending, ready: VecDeque<Task>, latest: HashMap<Uuid, Task>, misses, step, started, polled, done }`.

Each step:
1. Yield a ready task if there is one.
2. Otherwise, after the first poll, sleep `poll_delay(step)`, but never past the deadline.
3. Read the pending UUIDs (batched by `read`).
4. Move terminal tasks to `ready`, and count misses (3 in a row → `TaskNotFound`).
5. Reset `step` if anything finished.

Reaching the deadline yields `WaitTimeout` with the latest known state. Any error ends the stream.

`wait_for_results` collects `watch` and restores input order, so there's one polling implementation. Polling uses `tokio::time`. Dropping the stream drops the pending sleep or request, so polling stops immediately (FR-R2).

The planned `Clock` and `Transport` traits weren't needed. Tests drive real HTTP through wiremock and use millisecond poll intervals. Tokio's paused clock can't be used with real sockets, because it would fire the request timeouts while waiting on I/O.
