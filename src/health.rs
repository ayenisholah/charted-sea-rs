//! Health metrics: recent success rates across all Charted Sea customers, per scraper,
//! country, device and endpoint. Check them before a batch to avoid paying for blocked tasks.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::http::transport::ApiRequest;
use crate::types::Scraper;

const MAX_RANGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Start of a health query's time range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimeBound {
    /// The last hour (`LAST_1H`).
    Last1h,
    /// The last 24 hours (`LAST_24H`).
    Last24h,
    /// The last 72 hours (`LAST_72H`).
    Last72h,
    /// From this instant (inclusive). The range can't exceed 7 days.
    At(OffsetDateTime),
}

/// How results are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// One period for the whole range.
    Total,
    /// One period per hour.
    Hour,
    /// One period per day.
    Day,
}

impl Granularity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Total => "TOTAL",
            Self::Hour => "HOUR",
            Self::Day => "DAY",
        }
    }
}

/// A health query. Checked locally before sending (FR-7).
#[derive(Debug, Clone)]
#[must_use]
pub struct HealthQuery {
    start: TimeBound,
    end: Option<OffsetDateTime>,
    granularity: Granularity,
    scrapers: Vec<Scraper>,
    countries: Vec<String>,
    known_apis: Vec<String>,
}

impl HealthQuery {
    /// A query from `start` to now.
    pub fn new<S: Into<String>>(
        start: TimeBound,
        granularity: Granularity,
        scrapers: impl IntoIterator<Item = Scraper>,
        countries: impl IntoIterator<Item = S>,
    ) -> Self {
        Self {
            start,
            end: None,
            granularity,
            scrapers: scrapers.into_iter().collect(),
            countries: countries.into_iter().map(Into::into).collect(),
            known_apis: Vec::new(),
        }
    }

    /// End of the range (exclusive). Default: now.
    pub fn end(mut self, end: OffsetDateTime) -> Self {
        self.end = Some(end);
        self
    }

    /// Task metrics only: limit to these endpoints, e.g. `PRODUCT_SEARCH`, `PRODUCT_DETAIL`.
    pub fn known_apis<S: Into<String>>(mut self, apis: impl IntoIterator<Item = S>) -> Self {
        self.known_apis = apis.into_iter().map(Into::into).collect();
        self
    }

    fn query(
        &self,
        now: OffsetDateTime,
        with_known_apis: bool,
    ) -> Result<Vec<(&'static str, String)>> {
        if self.scrapers.is_empty() || self.countries.is_empty() {
            return Err(Error::local("scrapers and countries must not be empty"));
        }
        let start = match self.start {
            TimeBound::Last1h => "LAST_1H".to_owned(),
            TimeBound::Last24h => "LAST_24H".to_owned(),
            TimeBound::Last72h => "LAST_72H".to_owned(),
            TimeBound::At(at) => {
                let end = self.end.unwrap_or(now);
                if end <= at {
                    return Err(Error::local("the start must be before the end"));
                }
                if end - at > MAX_RANGE {
                    return Err(Error::local("the health range can't exceed 7 days"));
                }
                rfc3339(at)
            }
        };
        let names: Vec<&str> = self.scrapers.iter().map(Scraper::health_name).collect();
        let mut query = vec![("startAtIncl", start)];
        if let Some(end) = self.end {
            query.push(("endAtExcl", rfc3339(end)));
        }
        query.push(("granularity", self.granularity.as_str().to_owned()));
        query.push(("scrapers", names.join(",")));
        query.push(("countries", self.countries.join(",")));
        if with_known_apis && !self.known_apis.is_empty() {
            query.push(("knownApis", self.known_apis.join(",")));
        }
        Ok(query)
    }
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Session-preparation metric: can a browser or app session be opened without being blocked?
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SessionMetric {
    /// `SHOPEE`, `SHOPEE_ANDROID`, `LAZADA`, …
    pub scraper: String,
    /// Country code.
    pub country: String,
    /// `DESKTOP_WEB`, `MOBILE_WEB`, `SYNTHETIC`, …
    #[serde(default)]
    pub device: Option<String>,
    /// 0.0–1.0, or `None` without data.
    #[serde(default)]
    pub preparation_success_rate: Option<f64>,
    /// `HIGH` or `LOW` (treat LOW with caution).
    #[serde(default)]
    pub confidence: Option<String>,
    /// Average session duration.
    #[serde(default)]
    pub avg_duration_sec: Option<f64>,
    /// 95th percentile session duration.
    #[serde(default)]
    pub p95_duration_sec: Option<f64>,
    /// Average proxy traffic per session, in bytes.
    #[serde(default)]
    pub avg_proxy_bytes: Option<f64>,
    /// 95th percentile proxy traffic per session, in bytes.
    #[serde(default)]
    pub p95_proxy_bytes: Option<f64>,
    /// Every other field.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Scraping-task metric per endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TaskMetric {
    /// `SHOPEE`, `SHOPEE_ANDROID`, `LAZADA`, …
    pub scraper: String,
    /// Country code.
    pub country: String,
    /// Device type.
    #[serde(default)]
    pub device: Option<String>,
    /// `PRODUCT_SEARCH`, `PRODUCT_DETAIL`, …, or `UNKNOWN`.
    #[serde(default)]
    pub known_api: Option<String>,
    /// 0.0–1.0, or `None` without data.
    #[serde(default)]
    pub success_rate: Option<f64>,
    /// `HIGH` or `LOW`.
    #[serde(default)]
    pub confidence: Option<String>,
    /// Average task duration, submission to completion.
    #[serde(default)]
    pub avg_duration_sec: Option<f64>,
    /// 95th percentile task duration.
    #[serde(default)]
    pub p95_duration_sec: Option<f64>,
    /// Every other field.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One period of a health report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HealthPeriod<M> {
    /// Period start (inclusive, ISO 8601).
    pub start_at_incl: String,
    /// Period end (exclusive, ISO 8601).
    pub end_at_excl: String,
    /// Metric rows.
    pub metrics: Vec<M>,
}

/// A health-metrics response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HealthReport<M> {
    /// Range start.
    pub start_at_incl: String,
    /// Range end.
    pub end_at_excl: String,
    /// `TOTAL`, `HOUR` or `DAY`.
    pub granularity: String,
    /// Periods with their metric rows.
    pub periods: Vec<HealthPeriod<M>>,
    /// Every other field (`scrapers`, `countries`, `knownApis`, …).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl<M> HealthReport<M> {
    /// Every metric row across all periods.
    pub fn metrics(&self) -> impl Iterator<Item = &M> {
        self.periods.iter().flat_map(|p| p.metrics.iter())
    }
}

/// The health API. Get one with [`Client::health`].
#[derive(Debug, Clone)]
pub struct Health {
    client: Client,
}

impl Health {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Session-preparation success rates (`GET /health/session-metrics`).
    pub async fn session_metrics(
        &self,
        query: &HealthQuery,
    ) -> Result<HealthReport<SessionMetric>> {
        let params = query.query(OffsetDateTime::now_utc(), false)?;
        self.client
            .inner
            .send(&ApiRequest::get("/health/session-metrics".into(), params))
            .await
    }

    /// Scraping-task success rates per endpoint (`GET /health/scraping-task-metrics`).
    pub async fn task_metrics(&self, query: &HealthQuery) -> Result<HealthReport<TaskMetric>> {
        let params = query.query(OffsetDateTime::now_utc(), true)?;
        self.client
            .inner
            .send(&ApiRequest::get(
                "/health/scraping-task-metrics".into(),
                params,
            ))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn q(start: TimeBound) -> HealthQuery {
        HealthQuery::new(start, Granularity::Day, [Scraper::Shopee], ["SG"])
    }

    #[test]
    fn fr_7_rejects_bad_ranges_and_empty_lists_locally() {
        let now = datetime!(2026-06-11 00:00 UTC);
        let bad = [
            HealthQuery::new(TimeBound::Last1h, Granularity::Total, [], ["SG"]),
            HealthQuery::new(
                TimeBound::Last1h,
                Granularity::Total,
                [Scraper::Lazada],
                Vec::<String>::new(),
            ),
            q(TimeBound::At(datetime!(2026-06-03 00:00 UTC))),
            q(TimeBound::At(datetime!(2026-06-01 00:00 UTC)))
                .end(datetime!(2026-06-08 00:00:01 UTC)),
            q(TimeBound::At(datetime!(2026-06-05 00:00 UTC))).end(datetime!(2026-06-04 00:00 UTC)),
        ];
        for query in bad {
            let result = query.query(now, true);
            assert!(matches!(result, Err(Error::Validation { .. })));
        }
    }

    #[test]
    fn metrics_flattens_every_period() {
        let raw = serde_json::json!({
            "startAtIncl": "a", "endAtExcl": "b", "granularity": "HOUR",
            "periods": [
                { "startAtIncl": "a", "endAtExcl": "m", "metrics": [{ "scraper": "SHOPEE", "country": "SG" }] },
                { "startAtIncl": "m", "endAtExcl": "b", "metrics": [{ "scraper": "SHOPEE", "country": "ID" }] }
            ]
        });
        let tasks: HealthReport<TaskMetric> = serde_json::from_value(raw.clone()).unwrap();
        let sessions: HealthReport<SessionMetric> = serde_json::from_value(raw).unwrap();
        assert_eq!(
            tasks
                .metrics()
                .map(|m| m.country.as_str())
                .collect::<Vec<_>>(),
            ["SG", "ID"]
        );
        assert_eq!(sessions.metrics().count(), 2);
    }

    #[test]
    fn sends_every_shortcut_and_granularity_by_its_api_name() {
        let now = datetime!(2026-06-11 00:00 UTC);
        let q = HealthQuery::new(
            TimeBound::Last1h,
            Granularity::Total,
            [Scraper::Shopee],
            ["SG"],
        )
        .query(now, true)
        .unwrap();
        assert_eq!((q[0].1.as_str(), q[1].1.as_str()), ("LAST_1H", "TOTAL"));
        assert_eq!(Granularity::Day.as_str(), "DAY");
    }

    #[test]
    fn builds_the_documented_query() {
        let now = datetime!(2026-06-11 00:00 UTC);
        let exactly_7_days = q(TimeBound::At(datetime!(2026-06-04 00:00 UTC)))
            .query(now, true)
            .unwrap();
        assert_eq!(
            exactly_7_days[0],
            ("startAtIncl", "2026-06-04T00:00:00Z".to_owned())
        );
        let with_end = HealthQuery::new(
            TimeBound::Last72h,
            Granularity::Hour,
            [Scraper::ShopeeAndroid, Scraper::Lazada],
            ["SG", "ID"],
        )
        .end(now)
        .known_apis(["PRODUCT_SEARCH"]);
        assert_eq!(
            with_end.query(now, true).unwrap(),
            [
                ("startAtIncl", "LAST_72H".to_owned()),
                ("endAtExcl", "2026-06-11T00:00:00Z".to_owned()),
                ("granularity", "HOUR".to_owned()),
                ("scrapers", "SHOPEE_ANDROID,LAZADA".to_owned()),
                ("countries", "SG,ID".to_owned()),
                ("knownApis", "PRODUCT_SEARCH".to_owned()),
            ]
        );
        assert!(
            !with_end
                .query(now, false)
                .unwrap()
                .iter()
                .any(|(k, _)| *k == "knownApis")
        );
    }
}
