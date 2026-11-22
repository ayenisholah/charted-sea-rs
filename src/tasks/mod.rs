//! The tasks API: run, read, count, cancel, and waiting for results.

mod batch;
mod wait;

use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::time::Duration;

use std::collections::HashSet;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

pub use batch::UUID_BATCH_SIZE;
pub use wait::WaitOptions;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::http::transport::ApiRequest;
use crate::types::{CancelResponse, Scraper, Task, TaskCount, TaskPage, TaskRequest, TaskStatus};

const DEFAULT_AUTO_CANCEL: Duration = Duration::from_secs(600);
const DEFAULT_PAGE_SIZE: u32 = 20;
const SYNC_TIMEOUT_MARGIN: Duration = Duration::from_secs(30);
const RESERVED_BODY_KEYS: [&str; 5] = ["requests", "cacheMaxAgeInSec", "url", "method", "payload"];

/// The tasks API for one scraper. Get one with [`Client::tasks`].
#[derive(Debug, Clone)]
pub struct Tasks {
    client: Client,
    scraper: Scraper,
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn join<T: AsRef<str>>(items: &[T]) -> String {
    items
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(",")
}

impl Tasks {
    pub(crate) fn new(client: Client, scraper: Scraper) -> Self {
        Self { client, scraper }
    }

    fn path(&self, suffix: &str) -> Result<String> {
        let name = self.scraper.as_str();
        if name.trim().is_empty() {
            return Err(Error::local("scraper must not be empty"));
        }
        let segment: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
        Ok(format!("/scraping-tasks/{segment}{suffix}"))
    }

    /// Submits tasks. **Asynchronous by default** (`waitForCompletion=false`), unlike the API's
    /// default, because a dropped connection in synchronous mode cancels in-flight tasks.
    /// Never retried automatically. Await the returned builder to send.
    pub fn run(&self, requests: impl IntoIterator<Item = TaskRequest>) -> Run<Vec<Task>> {
        self.run_vec(requests.into_iter().collect())
    }

    fn run_vec(&self, requests: Vec<TaskRequest>) -> Run<Vec<Task>> {
        let body = (!requests.is_empty()).then(|| {
            let list = requests
                .into_iter()
                .map(|r| Value::Object(r.into_body()))
                .collect();
            Map::from_iter([("requests".to_owned(), Value::Array(list))])
        });
        Run::new(self.clone(), "/run", body)
    }

    /// Submits one task; the request's fields go at the body root. Same options as [`Tasks::run`].
    pub fn run_single(&self, request: TaskRequest) -> Run<Task> {
        Run::new(self.clone(), "/run-single", Some(request.into_body()))
    }

    /// Reads tasks. More than 100 UUIDs are split into batches and merged back in input order.
    pub async fn read(&self, query: ReadQuery) -> Result<Vec<Task>> {
        let path = self.path("")?;
        let Some(uuids) = query.uuids.as_deref() else {
            return self.read_batch(&path, &query, None).await;
        };
        let uuids = batch::unique(uuids);
        if uuids.len() > UUID_BATCH_SIZE && (query.limit.is_some() || query.offset.is_some()) {
            return Err(Error::local(format!(
                "limit and offset can't be combined with more than {UUID_BATCH_SIZE} uuids"
            )));
        }
        let mut found = Vec::new();
        for chunk in uuids.chunks(UUID_BATCH_SIZE) {
            found.extend(self.read_batch(&path, &query, Some(chunk)).await?);
        }
        Ok(batch::in_order(&uuids, found))
    }

    async fn read_batch(
        &self,
        path: &str,
        q: &ReadQuery,
        uuids: Option<&[Uuid]>,
    ) -> Result<Vec<Task>> {
        let mut query = Vec::new();
        if let Some(limit) = q
            .limit
            .or(uuids.map(|u| u32::try_from(u.len()).unwrap_or(u32::MAX)))
        {
            query.push(("limit", limit.to_string()));
        }
        if let Some(offset) = q.offset {
            query.push(("offset", offset.to_string()));
        }
        if let Some(uuids) = uuids {
            query.push((
                "uuids",
                join(&uuids.iter().map(Uuid::to_string).collect::<Vec<_>>()),
            ));
        }
        if !q.urls.is_empty() {
            query.push(("urls", join(&q.urls)));
        }
        if let Some(t) = q.min_pending_at {
            query.push(("minPendingAt", rfc3339(t)));
        }
        if let Some(t) = q.max_pending_at {
            query.push(("maxPendingAt", rfc3339(t)));
        }
        q.fields.push_query(&mut query);
        let page: TaskPage = self
            .client
            .inner
            .send(&ApiRequest::get(path.to_owned(), query))
            .await?;
        Ok(page.into_tasks())
    }

    /// Every task matching `query`, page by page (`offset` advances by `limit`, default 20),
    /// stopping at the first short page. A task seen twice while pages shift is yielded once.
    /// `uuids` in the query are ignored; use [`Tasks::read`] for those.
    #[must_use = "the stream does nothing until polled"]
    pub fn pages(&self, query: ReadQuery) -> BoxStream<'static, Result<Task>> {
        let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE).max(1);
        let start = PageState {
            tasks: self.clone(),
            query: ReadQuery {
                limit: Some(limit),
                offset: Some(query.offset.unwrap_or(0)),
                uuids: None,
                ..query
            },
            seen: HashSet::new(),
            done: false,
        };
        stream::unfold(start, PageState::next_page)
            .flat_map(stream::iter)
            .boxed()
    }

    /// Counts tasks by status and country.
    pub async fn count(&self, query: CountQuery) -> Result<Vec<TaskCount>> {
        let mut params = Vec::new();
        if !query.statuses.is_empty() {
            params.push((
                "statuses",
                join(
                    &query
                        .statuses
                        .iter()
                        .map(TaskStatus::as_str)
                        .collect::<Vec<_>>(),
                ),
            ));
        }
        if let Some(t) = query.min_pending_at {
            params.push(("minPendingAt", rfc3339(t)));
        }
        self.client
            .inner
            .send(&ApiRequest::get(self.path("/count")?, params))
            .await
    }

    /// Cancels tasks (and sub-tasks) by UUID and/or country. Refuses an empty request, so a
    /// mistake can't cancel everything. Never retried automatically.
    pub async fn cancel(&self, request: CancelRequest) -> Result<CancelResponse> {
        if request.uuids.is_empty() && request.countries.is_empty() {
            return Err(Error::local("cancel needs at least one uuid or country"));
        }
        let mut body = Map::new();
        if !request.uuids.is_empty() {
            body.insert("uuids".into(), serde_json::json!(request.uuids));
        }
        if !request.countries.is_empty() {
            body.insert("countries".into(), serde_json::json!(request.countries));
        }
        let req = ApiRequest {
            method: reqwest::Method::POST,
            path: self.path("/cancel")?,
            query: Vec::new(),
            body: Some(Value::Object(body)),
            idempotent: false,
            timeout: None,
        };
        self.client.inner.send(&req).await
    }

    /// Polls until every task is terminal, then returns one task per UUID in input order.
    /// Task outcomes (blocked, failed, cancelled) are data, not errors.
    pub async fn wait_for_results(
        &self,
        uuids: &[Uuid],
        options: WaitOptions,
    ) -> Result<Vec<Task>> {
        wait::wait_for_results(self.clone(), uuids, options).await
    }

    /// Streams each task as soon as a poll sees it finished (completion order). Dropping the
    /// stream stops polling. The stream is boxed, so `.next()` works without pinning.
    #[must_use = "the stream does nothing until polled"]
    pub fn watch(&self, uuids: &[Uuid], options: WaitOptions) -> BoxStream<'static, Result<Task>> {
        wait::watch(self.clone(), uuids, options).boxed()
    }
}

struct PageState {
    tasks: Tasks,
    query: ReadQuery,
    seen: HashSet<Uuid>,
    done: bool,
}

impl PageState {
    async fn next_page(mut self) -> Option<(Vec<Result<Task>>, Self)> {
        if self.done {
            return None;
        }
        let limit = self.query.limit.unwrap_or(DEFAULT_PAGE_SIZE);
        let page = match self.tasks.read(self.query.clone()).await {
            Ok(page) => page,
            Err(error) => {
                self.done = true;
                return Some((vec![Err(error)], self));
            }
        };
        self.done = page.len() < limit as usize;
        self.query.offset = self.query.offset.map(|o| o + limit);
        let seen = &mut self.seen;
        let fresh = page
            .into_iter()
            .filter(|t| t.uuid.is_none_or(|u| seen.insert(u)))
            .map(Ok)
            .collect();
        Some((fresh, self))
    }
}

#[derive(Debug, Clone, Default)]
struct Fields {
    all: bool,
    only: Vec<String>,
}

impl Fields {
    fn push_query(&self, query: &mut Vec<(&'static str, String)>) {
        if self.all {
            query.push(("includeAllFields", "true".into()));
        }
        if !self.only.is_empty() {
            query.push(("includeFields", join(&self.only)));
        }
    }
}

/// The non-generic part of a [`Run`], so it is compiled (and measured) once.
#[derive(Debug)]
struct RunSpec {
    tasks: Tasks,
    suffix: &'static str,
    body: Option<Map<String, Value>>,
    wait: bool,
    auto_cancel_after: Option<Duration>,
    cache_max_age: Option<Duration>,
    fields: Fields,
    params: Map<String, Value>,
}

impl RunSpec {
    fn into_request(self) -> Result<(Tasks, ApiRequest)> {
        let Some(mut body) = self.body else {
            return Err(Error::local("run needs at least one request"));
        };
        if let Some(key) = self
            .params
            .keys()
            .find(|k| RESERVED_BODY_KEYS.contains(&k.as_str()))
        {
            return Err(Error::local(format!(
                "scraper_param must not set \"{key}\""
            )));
        }
        if let Some(age) = self.cache_max_age {
            body.insert("cacheMaxAgeInSec".into(), age.as_secs().into());
        }
        body.extend(self.params);
        let mut query = vec![("waitForCompletion", self.wait.to_string())];
        if let Some(after) = self.auto_cancel_after {
            query.push(("autoCancelAfterSec", after.as_secs().to_string()));
        }
        self.fields.push_query(&mut query);
        let timeout = self
            .wait
            .then(|| self.auto_cancel_after.unwrap_or(DEFAULT_AUTO_CANCEL) + SYNC_TIMEOUT_MARGIN);
        let req = ApiRequest {
            method: reqwest::Method::POST,
            path: self.tasks.path(self.suffix)?,
            query,
            body: Some(Value::Object(body)),
            idempotent: false,
            timeout,
        };
        Ok((self.tasks, req))
    }
}

/// A pending `run` or `run_single`. Set options, then `.await` it.
#[must_use = "a Run does nothing until awaited"]
#[derive(Debug)]
pub struct Run<T> {
    spec: RunSpec,
    output: PhantomData<fn() -> T>,
}

impl<T> Run<T> {
    fn new(tasks: Tasks, suffix: &'static str, body: Option<Map<String, Value>>) -> Self {
        let spec = RunSpec {
            tasks,
            suffix,
            body,
            wait: false,
            auto_cancel_after: None,
            cache_max_age: None,
            fields: Fields::default(),
            params: Map::new(),
        };
        Self {
            spec,
            output: PhantomData,
        }
    }

    /// Keep the connection open until tasks finish (`waitForCompletion=true`). Default false.
    pub fn wait(mut self, wait: bool) -> Self {
        self.spec.wait = wait;
        self
    }

    /// Execution timeout for synchronous runs (`autoCancelAfterSec`; API default 600 s).
    pub fn auto_cancel_after(mut self, after: Duration) -> Self {
        self.spec.auto_cancel_after = Some(after);
        self
    }

    /// Return a cached successful result up to this age (not billed).
    pub fn cache_max_age(mut self, age: Duration) -> Self {
        self.spec.cache_max_age = Some(age);
        self
    }

    /// Return every field instead of the most important ones.
    pub fn include_all_fields(mut self) -> Self {
        self.spec.fields.all = true;
        self
    }

    /// Return only these fields.
    pub fn include_fields<S: Into<String>>(mut self, fields: impl IntoIterator<Item = S>) -> Self {
        self.spec.fields.only = fields.into_iter().map(Into::into).collect();
        self
    }

    /// A scraper-specific body parameter, e.g. `("productSearch_crawlNextPages", true.into())`.
    pub fn scraper_param(mut self, key: impl Into<String>, value: Value) -> Self {
        self.spec.params.insert(key.into(), value);
        self
    }
}

impl<T: DeserializeOwned + Send + 'static> IntoFuture for Run<T> {
    type Output = Result<T>;
    type IntoFuture = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        let prepared = self.spec.into_request();
        Box::pin(async move {
            let (tasks, req) = prepared?;
            tasks.client.inner.send(&req).await
        })
    }
}

/// Filters for [`Tasks::read`].
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct ReadQuery {
    limit: Option<u32>,
    offset: Option<u32>,
    uuids: Option<Vec<Uuid>>,
    urls: Vec<String>,
    min_pending_at: Option<OffsetDateTime>,
    max_pending_at: Option<OffsetDateTime>,
    fields: Fields,
}

impl ReadQuery {
    /// An empty query (API defaults: 20 tasks from the last 7 days).
    pub fn new() -> Self {
        Self::default()
    }

    /// Tasks with these UUIDs. Sets `limit` to the batch size unless you set one.
    pub fn uuids(mut self, uuids: impl IntoIterator<Item = Uuid>) -> Self {
        self.uuids = Some(uuids.into_iter().collect());
        self
    }

    /// Tasks for these URLs.
    pub fn urls<S: Into<String>>(mut self, urls: impl IntoIterator<Item = S>) -> Self {
        self.urls = urls.into_iter().map(Into::into).collect();
        self
    }

    /// Maximum tasks to return.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Tasks to skip.
    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Inclusive lower bound on when tasks were queued (API default: 7 days ago).
    pub fn min_pending_at(mut self, at: OffsetDateTime) -> Self {
        self.min_pending_at = Some(at);
        self
    }

    /// Exclusive upper bound on when tasks were queued.
    pub fn max_pending_at(mut self, at: OffsetDateTime) -> Self {
        self.max_pending_at = Some(at);
        self
    }

    /// Return every field.
    pub fn include_all_fields(mut self) -> Self {
        self.fields.all = true;
        self
    }

    /// Return only these fields.
    pub fn include_fields<S: Into<String>>(mut self, fields: impl IntoIterator<Item = S>) -> Self {
        self.fields.only = fields.into_iter().map(Into::into).collect();
        self
    }
}

/// Filters for [`Tasks::count`].
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct CountQuery {
    statuses: Vec<TaskStatus>,
    min_pending_at: Option<OffsetDateTime>,
}

impl CountQuery {
    /// All statuses from the last 7 days.
    pub fn new() -> Self {
        Self::default()
    }

    /// Only these statuses.
    pub fn statuses(mut self, statuses: impl IntoIterator<Item = TaskStatus>) -> Self {
        self.statuses = statuses.into_iter().collect();
        self
    }

    /// Inclusive lower bound on when tasks were queued.
    pub fn min_pending_at(mut self, at: OffsetDateTime) -> Self {
        self.min_pending_at = Some(at);
        self
    }
}

/// What [`Tasks::cancel`] should cancel.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct CancelRequest {
    uuids: Vec<Uuid>,
    countries: Vec<String>,
}

impl CancelRequest {
    /// An empty request; add UUIDs and/or countries.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel these tasks.
    pub fn uuids(mut self, uuids: impl IntoIterator<Item = Uuid>) -> Self {
        self.uuids = uuids.into_iter().collect();
        self
    }

    /// Cancel every task in these countries.
    pub fn countries<S: Into<String>>(mut self, countries: impl IntoIterator<Item = S>) -> Self {
        self.countries = countries.into_iter().map(Into::into).collect();
        self
    }
}
