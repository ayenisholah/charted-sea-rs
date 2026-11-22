# 0001: Async only, on reqwest and Tokio, rustls by default

**Status:** Accepted

## Context
The workload is waiting on network I/O and polling, often for many tasks at once. Most Rust services that would use this crate already run Tokio.

## Decision
The crate is async-only, built on `reqwest` with Tokio. TLS defaults to rustls (no OpenSSL to install or cross-compile); `native-tls` is an opt-in feature. Callers can pass their own `reqwest::Client` to share a connection pool.

## Consequences
- Sync users need a runtime (`tokio::runtime::Runtime::block_on`); a `blocking` feature can come later if asked for.
- The crate is tied to Tokio. That's an acceptable trade for a client whose users are overwhelmingly on Tokio.
