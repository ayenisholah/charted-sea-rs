//! API types. Unknown statuses, scrapers and fields are kept, never rejected (ADR 0002).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use uuid::Uuid;

/// A scraper path name. Unknown names pass through as [`Scraper::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Scraper {
    /// `shopee`
    Shopee,
    /// `shopee-android`
    ShopeeAndroid,
    /// `lazada`
    Lazada,
    /// Any other path name (e.g. a scraper added after this release).
    Other(String),
}

impl Scraper {
    /// The path segment used in task URLs.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Shopee => "shopee",
            Self::ShopeeAndroid => "shopee-android",
            Self::Lazada => "lazada",
            Self::Other(s) => s,
        }
    }
}

impl fmt::Display for Scraper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Scraper {
    fn from(s: &str) -> Self {
        match s {
            "shopee" => Self::Shopee,
            "shopee-android" => Self::ShopeeAndroid,
            "lazada" => Self::Lazada,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// A task's lifecycle status. `Unknown` keeps statuses this release doesn't know.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskStatus {
    /// Accepted and queued.
    Pending,
    /// Being executed.
    Running,
    /// Completed (billed).
    Success,
    /// Blocked; the server retries automatically.
    Blocked,
    /// Failed; the server retries automatically.
    Error,
    /// Blocked on every retry (not billed).
    BlockedTooManyTimes,
    /// Failed on every retry (not billed).
    ErrorTooManyTimes,
    /// Cancelled by the client or by timeout (not billed).
    Cancelled,
    /// Replaced by one or more derived tasks.
    Replaced,
    /// A status this release doesn't know. Treated as not finished.
    Unknown(String),
}

const STATUS_NAMES: [(&str, TaskStatus); 9] = [
    ("PENDING", TaskStatus::Pending),
    ("RUNNING", TaskStatus::Running),
    ("SUCCESS", TaskStatus::Success),
    ("BLOCKED", TaskStatus::Blocked),
    ("ERROR", TaskStatus::Error),
    ("BLOCKED_TOO_MANY_TIMES", TaskStatus::BlockedTooManyTimes),
    ("ERROR_TOO_MANY_TIMES", TaskStatus::ErrorTooManyTimes),
    ("CANCELLED", TaskStatus::Cancelled),
    ("REPLACED", TaskStatus::Replaced),
];

impl TaskStatus {
    /// The API's name for this status.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Unknown(s) => s,
            known => STATUS_NAMES
                .iter()
                .find(|(_, s)| s == known)
                .map_or("", |(n, _)| n),
        }
    }

    /// True when the task won't change again. `Blocked` and `Error` are not terminal (the
    /// server retries them); unknown statuses are treated as not terminal.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Success
                | Self::BlockedTooManyTimes
                | Self::ErrorTooManyTimes
                | Self::Cancelled
                | Self::Replaced
        )
    }
}

impl From<&str> for TaskStatus {
    fn from(s: &str) -> Self {
        STATUS_NAMES
            .iter()
            .find(|(n, _)| *n == s)
            .map_or_else(|| Self::Unknown(s.to_owned()), |(_, status)| status.clone())
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for TaskStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from(String::deserialize(deserializer)?.as_str()))
    }
}

/// HTTP method for a scraping request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    /// GET
    Get,
    /// POST, with a payload.
    Post,
}

/// One request to scrape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskRequest {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<Method>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
}

impl TaskRequest {
    /// A GET request for `url`.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: None,
            payload: None,
        }
    }

    /// A POST request for `url` with a JSON `payload`.
    pub fn post(url: impl Into<String>, payload: Value) -> Self {
        Self {
            url: url.into(),
            method: Some(Method::Post),
            payload: Some(payload),
        }
    }

    /// The URL to scrape.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The request as JSON object fields (for Run a Single Scraping Task's body).
    pub(crate) fn into_body(self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("url".into(), Value::String(self.url));
        if let Some(method) = self.method {
            map.insert(
                "method".into(),
                Value::String(
                    if method == Method::Post {
                        "POST"
                    } else {
                        "GET"
                    }
                    .into(),
                ),
            );
        }
        if let Some(payload) = self.payload {
            map.insert("payload".into(), payload);
        }
        map
    }
}

/// A scraping task as the API returns it. Fields this release doesn't know are kept in `extra`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Task {
    /// Present on asynchronous runs and reads.
    #[serde(default)]
    pub uuid: Option<Uuid>,
    /// The scraped URL.
    pub url: String,
    /// `GET` or `POST`, when the API returns it.
    #[serde(default)]
    pub method: Option<String>,
    /// The POST payload, when there was one.
    #[serde(default)]
    pub payload: Option<Value>,
    /// Lifecycle status.
    pub status: TaskStatus,
    /// Raw body from the scraped platform (JSON or HTML text).
    #[serde(default)]
    pub response_body: Option<String>,
    /// Why the task failed, when it did.
    #[serde(default)]
    pub error_message: Option<String>,
    /// Every other field, unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One row of Count Scraping Tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TaskCount {
    /// Country code, e.g. `ID`.
    pub country: String,
    /// Task status.
    pub status: TaskStatus,
    /// Number of tasks.
    pub count: u64,
}

/// Result of Cancel Scraping Tasks: everything cancelled, including sub-tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CancelResponse {
    /// Cancelled task UUIDs.
    #[serde(default)]
    pub uuids: Vec<Uuid>,
    /// Countries whose tasks were cancelled.
    #[serde(default)]
    pub countries: Vec<String>,
}

/// Read Scraping Tasks: the docs show a bare array, the live API returns a page object.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum TaskPage {
    List(Vec<Task>),
    Page { items: Vec<Task> },
}

impl TaskPage {
    pub(crate) fn into_tasks(self) -> Vec<Task> {
        match self {
            Self::List(tasks) | Self::Page { items: tasks } => tasks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn statuses_round_trip_and_unknown_ones_are_kept() {
        for (name, status) in &STATUS_NAMES {
            assert_eq!(&TaskStatus::from(*name), status);
            assert_eq!(serde_json::to_value(status).unwrap(), json!(name));
            assert_eq!(status.to_string(), *name);
        }
        let unknown: TaskStatus = serde_json::from_value(json!("PAUSED")).unwrap();
        assert_eq!(unknown, TaskStatus::Unknown("PAUSED".into()));
        assert!(!unknown.is_terminal());
        assert_eq!(unknown.as_str(), "PAUSED");
    }

    #[test]
    fn terminal_statuses_match_the_documented_lifecycle() {
        let terminal: Vec<_> = STATUS_NAMES
            .iter()
            .filter(|(_, s)| s.is_terminal())
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(
            terminal,
            [
                "SUCCESS",
                "BLOCKED_TOO_MANY_TIMES",
                "ERROR_TOO_MANY_TIMES",
                "CANCELLED",
                "REPLACED"
            ]
        );
    }

    #[test]
    fn scrapers_map_to_path_names() {
        for name in ["shopee", "shopee-android", "lazada", "daraz"] {
            assert_eq!(Scraper::from(name).to_string(), name);
        }
        assert_eq!(Scraper::from("daraz"), Scraper::Other("daraz".into()));
    }

    #[test]
    fn nfr_7_tasks_keep_unknown_fields_and_both_read_shapes_decode() {
        let raw = json!({ "url": "u", "status": "SUCCESS", "requestBody": null, "newField": 1 });
        let task: Task = serde_json::from_value(raw).unwrap();
        assert_eq!(task.extra.get("newField"), Some(&json!(1)));
        assert_eq!(task.response_body, None);
        let list: TaskPage =
            serde_json::from_value(json!([{ "url": "u", "status": "PENDING" }])).unwrap();
        let page: TaskPage = serde_json::from_value(
            json!({ "items": [{ "url": "u", "status": "PENDING" }], "total": 1 }),
        )
        .unwrap();
        assert_eq!(list.into_tasks(), page.into_tasks());
    }

    #[test]
    fn single_request_bodies_match_serialization() {
        for request in [
            TaskRequest::get("u"),
            TaskRequest::post("u", json!({ "a": 1 })),
            TaskRequest {
                method: Some(Method::Get),
                ..TaskRequest::get("u")
            },
        ] {
            assert_eq!(
                Value::Object(request.clone().into_body()),
                serde_json::to_value(request).unwrap()
            );
        }
    }

    #[test]
    fn requests_serialize_like_the_docs() {
        assert_eq!(
            serde_json::to_value(TaskRequest::get("u")).unwrap(),
            json!({ "url": "u" })
        );
        let post = TaskRequest::post("u", json!({ "a": 1 }));
        assert_eq!(post.url(), "u");
        assert_eq!(
            serde_json::to_value(post).unwrap(),
            json!({ "url": "u", "method": "POST", "payload": { "a": 1 } })
        );
    }
}
