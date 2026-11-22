## What and why

## Checklist

- [ ] Tests written first; `just check` passes locally (fmt, clippy, tests, docs, deny, 100% line coverage, fixtures)
- [ ] `docs/SPEC.md` updated if behaviour changed (and charted-sea-client's spec first, ADR 0004)
- [ ] New public types that may grow are `#[non_exhaustive]`; every public item has rustdoc
- [ ] No tokens, proxy credentials or scraped customer data in code, fixtures or logs
