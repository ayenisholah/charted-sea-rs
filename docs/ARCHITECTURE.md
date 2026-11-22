# Architecture

Planned.

```
src/
  lib.rs           re-exports and crate docs (quick start as a doctest)
  client.rs        Client, ClientBuilder
  config.rs        defaults and validation (pure)
  http/
    transport.rs   request building, auth header, timeout, error mapping (trait Transport for tests)
    retry.rs       backoff with full jitter, Retry-After (pure; jitter from an injected RNG)
    redact.rs      token and proxy-credential redaction (pure)
  tasks/
    mod.rs         Tasks: run, run_single, read, pages, count, cancel
    wait.rs        wait_for_results and watch (state machine over a Clock trait)
    batch.rs       UUID chunking and order-preserving merge (pure)
    status.rs      terminal-status table (pure)
  health.rs
  proxies/
    mod.rs         validate, read, update
    placeholder.rs ${rand(...)} / ${country(...)} parser (pure)
  error.rs
  types.rs
```

## Design rules

- `Client` is `Clone + Send + Sync` (an `Arc` inside) and reuses one `reqwest::Client` connection pool.
- Polling depends on a `Clock` trait (`tokio::time` in production, paused time in tests).
- The token is a `secrecy::SecretString`, exposed only when the `Authorization` header is built ([ADR 0003](adr/0003-secret-handling.md)).
- No `unsafe` (`#![forbid(unsafe_code)]`). No panics in library code: `clippy::unwrap_used` and `clippy::expect_used` are denied outside tests.
- Public types are `#[non_exhaustive]` where the API may grow, so new fields aren't breaking changes.

## Polling as a stream

`watch` is built with `futures::stream::unfold` over a state of `{ pending: Vec<Uuid>, ready: VecDeque<Task>, delay }`: it yields ready tasks first, and reads (in batches) only when the queue is empty. `wait_for_results` collects `watch` and restores input order, so there is one polling implementation.
