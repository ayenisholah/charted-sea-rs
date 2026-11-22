//! `watch` (a stream in completion order) and `wait_for_results` (input order), one implementation.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use futures::{Stream, StreamExt, stream};
use tokio::time::Instant;
use uuid::Uuid;

use super::batch::{in_order, unique};
use super::{ReadQuery, Tasks};
use crate::error::{Error, Result};
use crate::http::retry::poll_delay;
use crate::types::Task;

/// Consecutive reads a UUID may be missing from before [`Error::TaskNotFound`].
const MAX_MISSES: u32 = 3;

/// How [`Tasks::watch`] and [`Tasks::wait_for_results`] poll.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct WaitOptions {
    poll_interval: Duration,
    max_poll_interval: Duration,
    max_wait: Duration,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            max_poll_interval: Duration::from_secs(15),
            max_wait: Duration::from_secs(600),
        }
    }
}

impl WaitOptions {
    /// Shortest wait between polls (default 2 s).
    pub fn poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    /// Longest wait between polls (default 15 s).
    pub fn max_poll_interval(mut self, d: Duration) -> Self {
        self.max_poll_interval = d;
        self
    }

    /// Give up with [`Error::WaitTimeout`] after this long (default 10 min).
    pub fn max_wait(mut self, d: Duration) -> Self {
        self.max_wait = d;
        self
    }

    fn check(&self) -> Result<()> {
        if self.poll_interval.is_zero()
            || self.max_wait.is_zero()
            || self.poll_interval > self.max_poll_interval
        {
            return Err(Error::local(
                "need 0 < poll_interval ≤ max_poll_interval and max_wait > 0",
            ));
        }
        Ok(())
    }
}

struct State {
    tasks: Tasks,
    options: WaitOptions,
    order: Vec<Uuid>,
    pending: Vec<Uuid>,
    ready: VecDeque<Task>,
    latest: HashMap<Uuid, Task>,
    misses: HashMap<Uuid, u32>,
    step: u32,
    started: Instant,
    polled: bool,
    done: bool,
}

impl State {
    fn partial(&self) -> Vec<Task> {
        self.order
            .iter()
            .filter_map(|u| self.latest.get(u).cloned())
            .collect()
    }

    /// Applies one read; returns an error when UUIDs have gone missing too often.
    fn absorb(&mut self, tasks: Vec<Task>) -> Result<()> {
        let mut progressed = false;
        let mut seen = Vec::new();
        // `read` returns only the requested (pending) UUIDs, in order.
        for (uuid, task) in tasks.into_iter().filter_map(|t| t.uuid.map(|u| (u, t))) {
            seen.push(uuid);
            if task.status.is_terminal() {
                self.ready.push_back(task.clone());
                progressed = true;
            }
            self.latest.insert(uuid, task);
        }
        let mut not_found = Vec::new();
        for uuid in &self.pending {
            if seen.contains(uuid) {
                self.misses.remove(uuid);
            } else {
                let n = self.misses.entry(*uuid).or_default();
                *n += 1;
                if *n >= MAX_MISSES {
                    not_found.push(*uuid);
                }
            }
        }
        if !not_found.is_empty() {
            return Err(Error::TaskNotFound {
                uuids: not_found,
                partial: self.partial(),
            });
        }
        let latest = &self.latest;
        self.pending
            .retain(|u| !latest.get(u).is_some_and(|t| t.status.is_terminal()));
        self.step = if progressed { 0 } else { self.step + 1 };
        Ok(())
    }

    async fn next_or_end(self) -> Option<(Result<Task>, Self)> {
        if self.done { None } else { self.next().await }
    }

    async fn next(mut self) -> Option<(Result<Task>, Self)> {
        loop {
            if let Some(task) = self.ready.pop_front() {
                return Some((Ok(task), self));
            }
            if self.done || self.pending.is_empty() {
                return None;
            }
            if self.polled {
                let waited = self.started.elapsed();
                if waited >= self.options.max_wait {
                    self.done = true;
                    let error = Error::WaitTimeout {
                        waited,
                        unfinished: self.pending.len(),
                        partial: self.partial(),
                    };
                    return Some((Err(error), self));
                }
                let jitter = self.tasks.client.inner.jitter();
                let delay = poll_delay(
                    self.step,
                    self.options.poll_interval,
                    self.options.max_poll_interval,
                    jitter,
                );
                tokio::time::sleep(delay.min(self.options.max_wait.saturating_sub(waited))).await;
            }
            self.polled = true;
            let read = self
                .tasks
                .read(ReadQuery::new().uuids(self.pending.clone()))
                .await;
            if let Err(error) = read.and_then(|tasks| self.absorb(tasks)) {
                self.done = true;
                return Some((Err(error), self));
            }
        }
    }
}

pub(super) fn watch(
    tasks: Tasks,
    uuids: &[Uuid],
    options: WaitOptions,
) -> impl Stream<Item = Result<Task>> + Send + 'static {
    let order = unique(uuids);
    let invalid = options.check().err();
    let state = State {
        tasks,
        options,
        pending: order.clone(),
        order,
        ready: VecDeque::new(),
        latest: HashMap::new(),
        misses: HashMap::new(),
        step: 0,
        started: Instant::now(),
        polled: false,
        done: false,
    };
    let done = invalid.is_some();
    stream::iter(invalid.map(Err))
        .chain(stream::unfold(State { done, ..state }, State::next_or_end))
}

pub(super) async fn wait_for_results(
    tasks: Tasks,
    uuids: &[Uuid],
    options: WaitOptions,
) -> Result<Vec<Task>> {
    let mut finished = Vec::new();
    let mut stream = watch(tasks, uuids, options).boxed();
    while let Some(item) = stream.next().await {
        finished.push(item?);
    }
    Ok(in_order(uuids, finished))
}
