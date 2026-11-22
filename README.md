# charted-sea-rs

Async Rust client for the [Charted Sea](https://chartedsea.com) scraping API: submit scraping tasks, wait for results as a stream, read health metrics, and validate proxies. Same behaviour as [charted-sea-client](../charted-sea-client/README.md) for TypeScript.

> **Unofficial.** Not affiliated with or endorsed by Charted Sea.
> **Status: planned.** This README describes the 0.1.0 target. See the [roadmap](docs/ROADMAP.md).

<!-- Badges go live at M0: CI · coverage · crates.io · docs.rs · MSRV · OpenSSF Scorecard -->

## Install

```toml
[dependencies]
charted-sea = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Quick start

```rust
use charted_sea::{Client, Scraper, TaskRequest, WaitOptions};

#[tokio::main]
async fn main() -> Result<(), charted_sea::Error> {
    let client = Client::from_env()?; // reads CHARTED_SEA_TOKEN
    let tasks = client.tasks(Scraper::Shopee);

    let submitted = tasks
        .run([TaskRequest::get("https://shopee.sg/api/v4/search/search_items?keyword=tshirt")])
        .await?;

    let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();
    for task in tasks.wait_for_results(&uuids, WaitOptions::default()).await? {
        println!("{} {:?}", task.url, task.status);
    }
    Ok(())
}
```

Stream results as each task finishes:

```rust
use futures::StreamExt;

let mut finished = tasks.watch(&uuids, WaitOptions::default());
while let Some(task) = finished.next().await {
    let task = task?;
    println!("done: {} {:?}", task.url, task.status);
}
```

## Features

| Feature | Default | What it does |
|---|---|---|
| `rustls` | yes | TLS via rustls |
| `native-tls` | no | TLS via the platform library instead |
| `tracing` | no | `tracing` spans per request, with secrets redacted |

## Documentation

[Spec](docs/SPEC.md) · [Architecture](docs/ARCHITECTURE.md) · [Requirements](docs/REQUIREMENTS.md) · [Test strategy](docs/TEST_STRATEGY.md) · [CI/CD](docs/CI_CD.md) · [Release process](docs/RELEASE_PROCESS.md) · [Roadmap](docs/ROADMAP.md) · [ADRs](docs/adr/) · API docs on docs.rs (planned)

## Licence

[MIT](LICENSE)
