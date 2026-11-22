//! Unofficial async client for the [Charted Sea](https://chartedsea.com) scraping API.
//!
//! **Not affiliated with or endorsed by Charted Sea.**
//!
//! Submit scraping tasks, then wait for results or stream them as they finish. Runs are
//! asynchronous by default, polling backs off with jitter, reads of more than 100 UUIDs are
//! batched, and only idempotent calls are retried, so a dropped `run` is never sent twice.
//! Task outcomes (blocked, failed, cancelled) are data in [`Task::status`]; only faults are
//! [`Error`]s. Tokens and proxy credentials never appear in errors or `Debug` output.
//!
//! ```no_run
//! use charted_sea::{Client, Scraper, TaskRequest, WaitOptions};
//!
//! # async fn example() -> Result<(), charted_sea::Error> {
//! let client = Client::from_env()?; // reads CHARTED_SEA_TOKEN
//! let tasks = client.tasks(Scraper::Shopee);
//!
//! let submitted = tasks
//!     .run([TaskRequest::get("https://shopee.sg/api/v4/search/search_items?keyword=tshirt")])
//!     .cache_max_age(std::time::Duration::from_secs(3600))
//!     .await?;
//!
//! let uuids: Vec<_> = submitted.iter().filter_map(|t| t.uuid).collect();
//! for task in tasks.wait_for_results(&uuids, WaitOptions::default()).await? {
//!     println!("{} {}", task.url, task.status);
//! }
//! # Ok(())
//! # }
//! ```

mod client;
mod error;
mod http;
mod tasks;
mod types;

pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL};
pub use error::{Error, Result};
pub use http::redact::{REDACTED, redact};
pub use http::retry::RetryPolicy;
pub use tasks::{CancelRequest, CountQuery, ReadQuery, Run, Tasks, UUID_BATCH_SIZE, WaitOptions};
pub use time::OffsetDateTime;
pub use types::{CancelResponse, Method, Scraper, Task, TaskCount, TaskRequest, TaskStatus};
pub use uuid::Uuid;
