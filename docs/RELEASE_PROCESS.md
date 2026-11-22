# Release process

`main` only changes through pull requests, releases included.

## Versioning

SemVer, checked by `cargo semver-checks` before each release. Before 1.0, minor versions may break the API, and the changelog says how to migrate. The MSRV (now 1.89) can only rise in a minor release, noted in the changelog.

## How a release happens

1. Merge PRs with Conventional Commit titles (squash merge).
2. On a `release/X.Y.Z` branch, run `release-plz update`. It bumps `Cargo.toml` from the commit history, writes `CHANGELOG.md` and runs `cargo semver-checks`. Open a PR titled `chore: release X.Y.Z`.
3. Merging it runs `release-plz.yml`. Because `Cargo.toml` now holds a version that isn't on crates.io, it publishes through trusted publishing (OIDC; no stored token), tags `vX.Y.Z` and creates the GitHub release.
4. Check that docs.rs built the new version.

The release-PR step is done by hand, not by a bot: a PR opened by GitHub Actions wouldn't trigger the required checks under branch protection.

## Before the first publish (0.1.0)

- [ ] The GitHub repository exists and branch protection is on.
- [ ] Marc has agreed to the crate name `charted-sea`. crates.io names can't be reused once taken.
- [ ] M2 done: health, proxies, `pages`, the `tracing` feature.
- [x] Live smoke test passed (2026-11-22).
- [ ] Trusted publishing configured on crates.io for `release-plz.yml`, environment `crates-io`. The first publish has to be done by hand (`cargo publish`), because trusted publishing can only be added to an existing crate.
- [x] README says "unofficial" above the fold.
