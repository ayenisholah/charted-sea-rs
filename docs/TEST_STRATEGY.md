# Test strategy

| Level | Scope | Tooling | Runs |
|---|---|---|---|
| Unit | retry, redact, batch, status, placeholder parser, config | `cargo nextest` | every PR |
| Property | backoff bounds, batch/merge invariants, parser, redaction | proptest | every PR |
| Contract | every endpoint against a mock server answering from the shared fixtures | wiremock | every PR |
| Polling | `watch`/`wait_for_results` with paused time: progress, timeout, drop-cancel, unknown status | `tokio::test(start_paused = true)` | every PR |
| Doctests | README and rustdoc examples compile; network ones are `no_run` | `cargo test --doc` | every PR |
| Semver | public API compatibility vs the last release | cargo-semver-checks | PRs, release |
| Live smoke | one cached run + wait, one health query | `#[ignore]` test, `cargo test -- --ignored live_` | manual dispatch only |

## Shared fixtures

`tests/fixtures/api-reference/` is a copy of charted-sea-client's fixtures (the documented request and response examples). `fixtures.sha256` records their hash; a CI job fetches the client repo's fixture hash and fails on a mismatch, so both clients test against the same API examples.

## Coverage

`cargo llvm-cov --fail-under-lines 100` (branch coverage is nightly-only in Rust and isn't gated). Coverage is uploaded to Codecov from the Linux stable job.
