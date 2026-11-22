# Test strategy

50 tests; line coverage is 100% and gated. Run them all with `just check`.

| Level | Scope | Tooling | Where | Runs |
|---|---|---|---|---|
| Unit | retry and backoff, redact, batch, status table, type decoding, builder error mapping | `cargo nextest` | `#[cfg(test)]` modules in `src/` | every PR |
| Property | backoff ≤ `max_delay`, poll delay in range, merge restores order, chunks cover input once, a secret never survives `redact` | proptest | same | every PR |
| Contract | every built endpoint against the shared fixtures; the query string must match **exactly** (`ExactQuery`), and the body must equal the documented one | wiremock | `tests/contract.rs` | every PR |
| Polling | input and completion order, backoff, timeout with partial results, missing UUIDs, unknown statuses, drop-to-cancel, invalid options | wiremock + a scripted responder, millisecond intervals | `tests/polling.rs` | every PR |
| Errors and secrets | status → `Error`, retry policy, `Retry-After`, no retry for `run`, network and timeout errors, a sentinel token never in `Display`/`Debug` | wiremock | `tests/errors.rs` | every PR |
| Doctests | crate docs and `redact` examples compile; network ones are `no_run` | `cargo test --doc` | `src/` | every PR |
| README | the quick start runs against a mock server | wiremock | `tests/quickstart.rs` | every PR |
| Live smoke | one cached run + wait against the real API | `#[ignore]`, `just live` | `tests/live.rs` | manual only |

Requirement IDs (FR-*, NFR-*) appear in test names, so `cargo nextest run fr_3` finds the tests for one requirement.

## Shared fixtures

`tests/fixtures/api-reference/` is a copy of charted-sea-client's fixtures: the documented request and response examples, plus `read-tasks-live.json`, the page shape the live API actually returns.
- `just fixtures` checks the local files against `fixtures.sha256`.
- CI also checks that this hash equals the one in charted-sea-client's `main`, so both clients test against the same examples.
- To change fixtures, change them in charted-sea-client first, then copy them here.

## Coverage

`cargo llvm-cov nextest --fail-under-lines 100`, excluding `tests/` and `examples/`. Branch coverage needs nightly Rust and isn't gated.

Generic functions are kept as thin wrappers over non-generic ones. Otherwise every compiled copy of a generic is measured separately, and a branch that only one copy can take shows up as uncovered.

## Why real time, not a paused clock

The polling tests use real HTTP (wiremock) with poll intervals of a few milliseconds. With Tokio's paused clock, the runtime skips time forward whenever it's idle waiting on a socket, which would fire the 30 s request timeouts instantly.
