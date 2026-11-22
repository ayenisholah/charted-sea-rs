# CI/CD

Planned. Actions pinned to SHAs, read-only default permissions, and one local gate (`just check`) mirroring CI. The toolchain is pinned in `rust-toolchain.toml`.

| File | Trigger | What it does |
|---|---|---|
| `ci.yml` | push to `main`, every PR | `lint`: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `typos`, conventional-commit check on PRs. `test`: `cargo nextest run` + doctests on {ubuntu, macos, windows} × stable, and ubuntu × MSRV; feature powerset with `cargo hack --feature-powerset check`. `coverage`: `cargo llvm-cov --fail-under-lines 100`, upload to Codecov. `docs`: `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`. `fixtures`: shared fixture hash check. |
| `audit.yml` | PR, push, daily | `cargo deny check` (advisories, licences, bans, sources) and `cargo machete`. |
| `security.yml` | PR, push | gitleaks. |
| `scorecard.yml` | weekly | OpenSSF Scorecard. |
| `release-plz.yml` | push to `main` | release-plz opens a release PR (version bump from Conventional Commits, `CHANGELOG.md`, `cargo semver-checks`); on merge, publishes to crates.io via trusted publishing (OIDC, no stored token), tags `vX.Y.Z` and creates the GitHub release. |
| `live-smoke.yml` | `workflow_dispatch` only | `cargo test -- --ignored live_` in the protected `live` environment with `CHARTED_SEA_TOKEN`. |

## Local gate

```make
# justfile (planned)
check: fmt clippy test doc deny cov
fmt:    cargo fmt --check
clippy: cargo clippy --all-targets --all-features -- -D warnings
test:   cargo nextest run --all-features && cargo test --doc --all-features
doc:    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
deny:   cargo deny check && cargo machete
cov:    cargo llvm-cov nextest --all-features --fail-under-lines 100
```

## Caching

`Swatinem/rust-cache` keyed on `Cargo.lock` and toolchain; `cargo-binstall` for tool installs so CI doesn't compile them.

## Dependabot

Weekly for `cargo` and `github-actions`. Nothing auto-merges.
