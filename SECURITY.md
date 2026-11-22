# Security policy

## Supported versions

Only the latest minor release gets security fixes until 1.0. After 1.0, the latest two minor releases do.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting (**Security → Report a vulnerability**) on this repository. Please don't open a public issue. Expect an acknowledgement within 3 working days and a fix or mitigation plan within 14.

## How secrets are handled

- **API token.** Read from `CHARTED_SEA_TOKEN` (or passed in explicitly). It is never logged, never included in error messages, and redacted from any debug output (the token type is `secrecy::SecretString`, so `Debug` prints `[REDACTED]`).
- **Proxy URLs** contain proxy-provider usernames and passwords. Every error, log line and debug view replaces the userinfo part with `***`.
- **Repository.** `.env` is git-ignored. CI runs gitleaks on every push. Test fixtures use obviously fake tokens such as `cs_test_000`.
- **CI.** Pull-request workflows never receive the live token. The live smoke test runs only by manual dispatch, from `main`, with the `CHARTED_SEA_TOKEN` repository secret.

## Supply chain

Dependencies are updated by Dependabot and checked by `cargo deny` (advisories, licences, bans) and `cargo audit`. Releases are built in GitHub Actions and published with crates.io trusted publishing, so every published version links to the commit and workflow that built it.
