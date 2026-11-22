# 0004: Same semantics as the TypeScript client

**Status:** Accepted

## Context
Two clients for one API drift unless something holds them together, and users comparing them would rightly expect the same behaviour.

## Decision
- The TypeScript spec is the behavioural reference: defaults, status table, polling, retry policy, error categories. This crate's spec lists only the differences.
- Both repos test against the same fixture files, with a CI hash check.
- Requirement IDs are shared, so `FR-3` means the same thing in both test suites.

## Consequences
- Behaviour changes land in the TypeScript spec first, then here.
- Idiomatic differences (streams, drop-to-cancel, builders) are allowed and listed in the spec.
