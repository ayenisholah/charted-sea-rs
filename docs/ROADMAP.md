# Roadmap

Starts after charted-sea-client M2, so the fixtures and spec are settled.

## M0: Scaffold and CI (0.5 day) ✅ done
`Cargo.toml` (edition 2024, `rust-version` 1.89), lints in `Cargo.toml`, justfile, all workflows, fixtures copied with hash.
**Done when:** `redact` and `status` modules with tests merge through a PR, every CI job is green on all three OSes, and a planted `unwrap()` or 99% coverage fails the gate.

## M1: Core tasks (1.5 days) ✅ done
`Client`, builder, transport, errors, retry policy, `run`/`run_single`/`read`/`count`/`cancel`, `watch`, `wait_for_results`.
**Done when:** shared FR-1 to FR-5, FR-R1, FR-R2 and NFR-1 to NFR-3 pass at 100% line coverage, and the README quick start runs as an example against wiremock. *Met: 50 tests at 100% line coverage; `tests/quickstart.rs`; the live smoke test passed.*

## M2: Full surface and docs (1 day) ✅ done
`pages`, health, proxies, `ProxyUrl::parse`, `tracing` feature, `examples/health_gated_batch.rs` and `stream_to_jsonl.rs`, crate-level docs, live smoke run. The health-gated example ran live and correctly declined to submit when Shopee SG search was at 0.0.
**Done when:** every endpoint has a contract test, `cargo doc` is warning-free with every public item documented, and the live smoke workflow has passed once.

## M3: Release 0.1.0 (0.5 day)
**Done when:** the [release checklist](RELEASE_PROCESS.md#before-the-first-publish-010) is complete and docs.rs shows 0.1.0.

## Later
- Optional `blocking` feature if users ask.
- Follow REPLACED tasks (open question 4).
