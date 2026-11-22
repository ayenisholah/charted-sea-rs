# charted-sea

Async Rust client for the [Charted Sea](https://chartedsea.com) scraping API. Submit scraping tasks for Shopee and Lazada, then wait for the results or stream them as each one finishes.

> **Unofficial.** Not affiliated with or endorsed by Charted Sea.
> **Status: pre-release (0.x).** The tasks API is complete and tested against the real API. Health metrics and proxy endpoints are planned (see the [roadmap](docs/ROADMAP.md)). It isn't published to crates.io yet; install it from Git for now.

## What it solves

Charted Sea scrapes marketplace pages for you: you send URLs, it returns the raw responses. A production integration has to get several things right, and each one costs time or money when it goes wrong:

- **Long runs.** The API can hold a connection open until tasks finish, but a dropped connection cancels the tasks. Charted Sea recommends submitting asynchronously and polling instead. This crate does that by default.
- **Polling.** Checking every task in a tight loop wastes requests. The crate backs off with jitter, speeds up again when tasks finish, and stops at a deadline.
- **Batches.** A read can carry about 100 task IDs at most. The crate splits bigger lists and returns results in the order you asked for.
- **Paying twice.** Successful tasks are billed. If the network fails while a `run` is being sent, retrying could submit the same tasks again. The crate never retries `run`, `run_single` or `cancel`; it tells you the request *may have been submitted*, so you can re-run with a cache age and get the cached result for free.
- **Failures that aren't errors.** In a batch of 100, a few blocked pages are normal. Task outcomes (`BlockedTooManyTimes`, `ErrorTooManyTimes`, `Cancelled`) come back as data in `Task::status`. Only faults (bad token, server down, timeout) are `Err`.
- **Leaked secrets.** The API token is a `secrecy::SecretString`. It never appears in `Debug`, `Display`, error chains or logs, and neither do credentials inside proxy URLs.
- **API drift.** New statuses, scrapers and fields don't break deserialisation. They're kept as `Unknown(..)` / `Other(..)` variants and an `extra` map. The live API's read response already differs from the docs, and the crate accepts both shapes.

## Install

The crate isn't on crates.io yet. Use it from GitHub:

```toml
[dependencies]
charted-sea = { git = "https://github.com/ayenisholah/charted-sea-rs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures = "0.3" # only if you use `watch`
```

You need Rust **1.89 or newer** and a Tokio runtime.

| Feature | Default | What it does |
|---|---|---|
| `rustls` | yes | TLS through rustls; no OpenSSL to install |
| `native-tls` | no | TLS through the platform library instead (`default-features = false, features = ["native-tls"]`) |

You need an API token: log in to the [Charted Sea console](https://console.chartedsea.com), then choose **Get API Token → Generate Token**. Tokens are valid for 365 days.

```sh
export CHARTED_SEA_TOKEN=...
```

## Usage

### Submit tasks and wait for the results

```rust
use std::time::Duration;
use charted_sea::{Client, Scraper, TaskRequest, TaskStatus, WaitOptions};

#[tokio::main]
async fn main() -> Result<(), charted_sea::Error> {
    let client = Client::from_env()?; // reads CHARTED_SEA_TOKEN
    let tasks = client.tasks(Scraper::Shopee);

    let submitted = tasks
        .run([TaskRequest::get("https://shopee.sg/api/v4/search/search_items?keyword=tshirt")])
        .cache_max_age(Duration::from_secs(3600)) // a result up to an hour old is free
        .await?;

    let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();
    for task in tasks.wait_for_results(&uuids, WaitOptions::default()).await? {
        match task.status {
            TaskStatus::Success => println!("{}: {} bytes", task.url, task.response_body.unwrap_or_default().len()),
            other => println!("{}: {other} {}", task.url, task.error_message.unwrap_or_default()),
        }
    }
    Ok(())
}
```

`wait_for_results` returns one task per UUID, in the order you passed them, once every task has finished. `response_body` is the platform's raw JSON or HTML as a string; parse it however you like.

### Stream results as they finish

```rust
use futures::StreamExt;

let mut finished = tasks.watch(&uuids, WaitOptions::default());
while let Some(task) = finished.next().await {
    let task = task?;
    println!("done: {} {}", task.url, task.status);
}
```

Dropping the stream stops polling straight away. There's no cancel token to manage.

### Other calls

```rust
use charted_sea::{CancelRequest, CountQuery, ReadQuery, TaskStatus};

// Read tasks by UUID (batched past 100), URL or time range.
let found = tasks.read(ReadQuery::new().uuids(uuids.clone())).await?;

// Count what's still in flight, by status and country.
let counts = tasks
    .count(CountQuery::new().statuses([TaskStatus::Pending, TaskStatus::Running]))
    .await?;

// Cancel by UUID and/or country. An empty request is refused.
tasks.cancel(CancelRequest::new().uuids(uuids)).await?;

// POST requests and scraper-specific parameters.
let page = tasks
    .run([TaskRequest::get("https://shopee.co.id/api/v4/search/search_items?keyword=tshirt")])
    .scraper_param("productSearch_crawlNextPages", true.into())
    .scraper_param("productSearch_crawlNextPages_maxPages", 3.into())
    .await?;
```

### Configuration

```rust
use std::time::Duration;
use charted_sea::{Client, RetryPolicy, WaitOptions};

let client = Client::builder()
    .token(std::env::var("CHARTED_SEA_TOKEN").unwrap_or_default())
    .timeout(Duration::from_secs(30))                    // per request; synchronous runs get auto-cancel + 30 s
    .retry(RetryPolicy { max_attempts: 3, base_delay: Duration::from_millis(500), max_delay: Duration::from_secs(10) })
    .user_agent_suffix("my-app/1.0")
    .http_client(reqwest::Client::new())                 // optional: share your own pool, proxy or TLS settings
    .build()?;

let wait = WaitOptions::default()
    .poll_interval(Duration::from_secs(2))
    .max_poll_interval(Duration::from_secs(15))
    .max_wait(Duration::from_secs(600));
```

| Default | Value |
|---|---|
| Run mode | asynchronous (`waitForCompletion=false`); `.wait(true)` for synchronous |
| Request timeout | 30 s |
| Retries (reads, counts only) | 3 attempts, full-jitter backoff from 500 ms, capped at 10 s, `Retry-After` honoured |
| Polling | 2 s, growing to 15 s while nothing finishes, for at most 10 min |

### Errors

```rust
use charted_sea::Error;

match tasks.run([TaskRequest::get(url)]).await {
    Ok(submitted) => { /* … */ }
    Err(Error::Auth { .. }) => eprintln!("check CHARTED_SEA_TOKEN"),
    Err(Error::RateLimited { retry_after, .. }) => eprintln!("slow down: {retry_after:?}"),
    Err(Error::Network { maybe_submitted: true, .. } | Error::Timeout { maybe_submitted: true, .. }) => {
        // The tasks may exist. Re-run with .cache_max_age(..) to get them without paying twice.
    }
    Err(other) => eprintln!("{other}"),
}
```

`wait_for_results` adds `Error::WaitTimeout { partial, .. }` (the latest state of every task, in input order) and `Error::TaskNotFound { uuids, .. }`.

## Examples

```sh
CHARTED_SEA_TOKEN=... cargo run --example stream_to_jsonl -- tshirt hoodie > tasks.jsonl
```

[`examples/stream_to_jsonl.rs`](examples/stream_to_jsonl.rs) submits search pages and writes each finished task as one JSON line.

## How it's tested

- **50 tests and 100% line coverage**, gated in CI.
- Every request is checked against the examples in Charted Sea's API reference. These are the same fixture files as [charted-sea-client](https://github.com/ayenisholah/charted-sea-client), and CI checks their hashes match, so the Rust and TypeScript clients can't drift apart.
- Property tests cover backoff bounds, batching and redaction.
- A live smoke test runs against the real API by hand (`just live`).

## Documentation

| Doc | What's in it |
|---|---|
| [Spec](docs/SPEC.md) | Public API, types, errors, and how this crate differs from the TypeScript client |
| [Architecture](docs/ARCHITECTURE.md) | Modules and design rules |
| [Requirements](docs/REQUIREMENTS.md) | Numbered requirements; each one has a test with its ID in the name |
| [Test strategy](docs/TEST_STRATEGY.md) | Unit, property, contract, polling and live tests |
| [CI/CD](docs/CI_CD.md) and [release process](docs/RELEASE_PROCESS.md) | Workflows, gates, and how a release reaches crates.io |
| [Roadmap](docs/ROADMAP.md) | What's done and what's next |
| [ADRs](docs/adr/) | Recorded decisions |
| [Contributing](CONTRIBUTING.md) and [security](SECURITY.md) | How to work on it, how to report a vulnerability |

API docs: `cargo doc --open`. docs.rs follows the first crates.io release.

## Related

- [charted-sea-client](https://github.com/ayenisholah/charted-sea-client): the same client in TypeScript (on npm).
- [charted-sea-mcp](https://github.com/ayenisholah/charted-sea-mcp): an MCP server, so AI agents can check platform health and run scraping tasks.

## Licence

[MIT](LICENSE)
