# 0002: Forward-compatible types

**Status:** Accepted

## Context
Charted Sea adds scrapers, countries and fields over time. If a strict Rust enum meets an unknown status, deserialisation fails and the caller loses the whole batch.

## Decision
- Enums from the API (`TaskStatus`, `Scraper`, `Country`, `Device`, `KnownApi` …) are `#[non_exhaustive]` and have an `Unknown(String)` / `Other(String)` variant via a custom `Deserialize`.
- Response structs keep unrecognised fields in `extra: serde_json::Map` (`#[serde(flatten)]`).
- Unknown statuses are non-terminal, matching the TypeScript client.

## Consequences
- Callers must have a wildcard match arm, which `#[non_exhaustive]` enforces.
- Adding a known variant later is not a breaking change.
