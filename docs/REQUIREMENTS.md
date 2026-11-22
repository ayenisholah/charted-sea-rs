# Requirements

Functional requirements FR-1 to FR-9 are shared with [charted-sea-client](../../charted-sea-client/docs/REQUIREMENTS.md) and tested here with the same IDs. Rust-specific additions:

- **FR-R1** `watch` yields each task exactly once, as soon as a poll sees it terminal.
- **FR-R2** Dropping a `watch` stream or `wait_for_results` future stops polling with no further requests.

## Non-functional

- **NFR-1 Secrets.** As in the TypeScript client; `Debug` on `Client`, `ClientBuilder` and every error prints `[REDACTED]`.
- **NFR-2 No double submission** and **NFR-3 bounded retries**, as in the TypeScript client.
- **NFR-R1 Safety.** `#![forbid(unsafe_code)]`; no `unwrap`/`expect`/panics in library code (Clippy-enforced).
- **NFR-R2 Semver.** `cargo semver-checks` passes against the last release.
- **NFR-R3 Dependencies.** `cargo deny` passes (advisories, MIT/Apache-compatible licences, no duplicate major versions of core crates); no unused dependencies (`cargo machete`).
- **NFR-R4 Portability.** Tests pass on Linux, macOS and Windows on stable, and on Linux on MSRV.
- **NFR-R5 Docs.** Every public item has rustdoc (`#![deny(missing_docs)]`); examples compile as doctests.
- **NFR-R6 Coverage.** 100% line coverage (`cargo llvm-cov --fail-under-lines 100`), excluding `examples/`.
