# Every gate, the same as CI. Run before opening a PR.
check: fmt clippy test doc deny cov fixtures

fmt:
    cargo fmt --check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo nextest run --all-features
    cargo test --doc --all-features

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

deny:
    cargo deny check
    cargo machete

cov:
    cargo llvm-cov nextest --all-features --ignore-filename-regex 'tests/|examples/' --lcov --output-path lcov.info
    scripts/check-coverage.sh lcov.info

# The fixtures must match charted-sea-client's (same API examples in both test suites).
fixtures:
    cd tests/fixtures && find api-reference -name '*.json' | LC_ALL=C sort | while read f; do printf '%s\0' "$(basename "$f")"; tr -d '\r' < "$f"; printf '\0'; done | sha256sum | cut -d' ' -f1 | diff - fixtures.sha256

# Live smoke test; needs CHARTED_SEA_TOKEN. Never in PR CI.
live:
    cargo test --test live -- --ignored --nocapture
