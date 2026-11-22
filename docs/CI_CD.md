# CI/CD

Every third-party action is pinned to a full commit SHA, with the version in a comment. Workflows default to `permissions: contents: read`. The toolchain is pinned in `rust-toolchain.toml`. `just check` runs the same gates locally.

## Workflows

| File | Trigger | What it does |
|---|---|---|
| `ci.yml` | push to `main`, every PR | `lint`: `cargo fmt --check`, `clippy --all-targets --all-features -D warnings`, `typos`, and `cargo hack --feature-powerset check`. `test`: `cargo nextest` + doctests on Ubuntu, macOS and Windows. `msrv`: `cargo +1.89 check --lib`. `coverage`: `cargo llvm-cov --fail-under-lines 100`, uploaded to Codecov. `docs`: `cargo doc` with `-D warnings`. `fixtures`: the local hash, and that it equals charted-sea-client's. |
| `audit.yml` | PR, push, daily | `cargo deny check` (advisories, licences, bans, sources) and `cargo machete` (unused dependencies). |
| `security.yml` | PR, push | gitleaks over the full history (`.gitleaks.toml` allows only the obviously fake test tokens). |
| `scorecard.yml` | weekly, push to `main` | OpenSSF Scorecard. |
| `release-plz.yml` | push to `main` | If `Cargo.toml` holds a version that isn't on crates.io: publish via crates.io trusted publishing (OIDC, environment `crates-io`), tag `vX.Y.Z` and create the GitHub release. Otherwise it does nothing. |
| `live-smoke.yml` | `workflow_dispatch` only | `cargo test --test live -- --ignored` in the protected `live` environment with the `CHARTED_SEA_TOKEN` secret. |

## Branch protection (planned for when the repo is on GitHub)

`main` accepts only pull requests, admins included. Required checks: `lint`, `test (ubuntu-latest)`, `test (macos-latest)`, `test (windows-latest)`, `msrv`, `coverage`, `docs`, `fixtures`, `deny`, `gitleaks`. Squash merges only, with linear history.

## Local gate

```make
# justfile
check: fmt clippy test doc deny cov fixtures
fmt:      cargo fmt --check
clippy:   cargo clippy --all-targets --all-features -- -D warnings
test:     cargo nextest run --all-features && cargo test --doc --all-features
doc:      RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
deny:     cargo deny check && cargo machete
cov:      cargo llvm-cov nextest --all-features --fail-under-lines 100 --ignore-filename-regex 'tests/|examples/'
fixtures: hash of tests/fixtures/api-reference == tests/fixtures/fixtures.sha256
live:     cargo test --test live -- --ignored --nocapture   # needs CHARTED_SEA_TOKEN
```

`cargo-machete` is pinned to 0.8.0, because later releases need an unstable standard-library feature.

## Caching

`Swatinem/rust-cache` is keyed on `Cargo.lock` and the toolchain. `taiki-e/install-action` installs prebuilt tools, so CI doesn't compile them.

## Dependabot

Weekly for `cargo` and `github-actions`. Nothing auto-merges.
