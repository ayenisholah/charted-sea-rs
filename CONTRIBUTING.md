# Contributing

Thanks for helping. This is an unofficial project and not affiliated with Charted Sea.

## Setup

```sh
rustup show           # installs the toolchain from rust-toolchain.toml
cargo install --locked just cargo-nextest cargo-llvm-cov cargo-deny cargo-hack
cargo install --locked cargo-machete --version 0.8.0   # later versions need nightly
just check            # every gate; must pass before a PR
```

## Workflow

1. Open an issue first for anything bigger than a bug fix, so we agree on the approach.
2. Branch from `main` (`feat/…`, `fix/…`, `docs/…`).
3. Write the failing test first. See [test strategy](docs/TEST_STRATEGY.md).
4. Run `just check`. CI runs the same gate and blocks the merge if it fails.
5. Use [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:` …). release-plz builds the version and changelog from them.
6. Note user-visible changes in the PR description; release-plz writes the changelog.

## Rules that reviews enforce

- Behaviour matches [the spec](docs/SPEC.md). If the API behaves differently, update the spec in the same PR and link the evidence.
- Line coverage stays at 100%. Don't add coverage exclusions to get past the gate.
- No real tokens, proxy credentials, or customer data in code, fixtures or issues.
- Docs state what is true now. Mark anything unbuilt as planned.
