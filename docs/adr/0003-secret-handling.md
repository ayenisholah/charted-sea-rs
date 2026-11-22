# 0003: Secrets as `SecretString`, redacted everywhere

**Status:** Accepted

## Context
Tokens leak through `Debug` output, error chains, panics and `tracing` fields. Proxy URLs carry provider passwords in their userinfo.

## Decision
- The token is a `secrecy::SecretString`, exposed only when building the `Authorization` header.
- `Client`, `ClientBuilder` and `ProxyUrl` implement `Debug` by hand, printing `[REDACTED]` for secrets and `***` for URL userinfo.
- `reqwest::Error` is wrapped with its URL stripped (`without_url()`) before it goes into `Error`.
- The `tracing` feature records endpoint and status, never headers or bodies.

## Consequences
- A sentinel-token test runs every error path and asserts the token never appears in `Display`, `Debug` or captured traces.
