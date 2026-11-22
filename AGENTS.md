# charted-sea-rs agent instructions

Unofficial async Rust client for the Charted Sea scraping API. Tokio + reqwest, rustls by default.

## Commands

```sh
just check                               # every gate; run before opening a PR
cargo nextest run --all-features         # tests
cargo llvm-cov nextest --fail-under-lines 100
just live                                # live smoke; needs CHARTED_SEA_TOKEN; never in PR CI
```

## Repo map

| Path | What it is |
|---|---|
| `src/http` | transport, retry, redaction |
| `src/tasks` | task methods, `Run<T>` builder, polling stream (`wait.rs`), batching |
| `src/types.rs` | API types; the status table is `TaskStatus::is_terminal` |
| `src/health.rs`, `src/proxies` | planned (M2): health metrics; proxy endpoints and placeholder parser |
| `tests/` | contract, polling, errors, quick start, live (`#[ignore]`) |
| `tests/fixtures/api-reference` | shared with charted-sea-client; don't edit here (`just fixtures` checks the hash) |

## Rules: never weaken these

- Behaviour follows the TypeScript spec ([ADR 0004](docs/adr/0004-shared-semantics-with-typescript.md)); differences are listed in [docs/SPEC.md](docs/SPEC.md).
- Never auto-retry `run`, `run_single`, `cancel` or proxy `update`.
- Secrets stay in `SecretString` and are redacted in every `Debug`/`Display` ([ADR 0003](docs/adr/0003-secret-handling.md)).
- No `unsafe`, no `unwrap`/`expect`/panics in library code. Don't add `#[allow]` to get past Clippy.
- New public types that may grow are `#[non_exhaustive]`.
- Write the failing test first; line coverage stays at 100%. Keep generic functions thin wrappers over non-generic ones (see docs/TEST_STRATEGY.md).
- `main` only changes through PRs. Never push to it directly.
- Docs state what's true now. Mark anything unbuilt as planned.
