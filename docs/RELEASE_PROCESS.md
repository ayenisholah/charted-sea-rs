# Release process

## Versioning

SemVer, checked by `cargo semver-checks` in the release PR. Before 1.0, minor versions may break the API. The MSRV can rise only in a minor release, noted in the changelog.

## How a release happens

1. Merge PRs with Conventional Commit titles (squash merge).
2. release-plz opens or updates a release PR: bumped `Cargo.toml` version, `CHANGELOG.md`, and a semver check.
3. Merging it publishes to crates.io through trusted publishing, tags `vX.Y.Z` and creates the GitHub release.
4. Check docs.rs built the new version.

## Before the first publish (0.1.0)

- [ ] Marc has agreed to the crate name `charted-sea` (crates.io names can't be reused once taken).
- [ ] M2 done: full surface, docs, examples.
- [ ] Live smoke test passed once.
- [ ] crates.io trusted publishing configured for `release-plz.yml`.
- [ ] README says "unofficial" above the fold.
