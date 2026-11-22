//! Backoff, jitter and `Retry-After` (pure).

use std::time::{Duration, SystemTime};

/// Retry policy for idempotent calls. `run`, `run_single`, `cancel` and proxy `update` are never retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, including the first.
    pub max_attempts: u32,
    /// Base for exponential backoff.
    pub base_delay: Duration,
    /// No single wait is longer than this. A `Retry-After` beyond it is not waited out.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
        }
    }
}

/// Statuses worth retrying on an idempotent call.
pub(crate) fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

fn scale(d: Duration, factor: f64) -> Duration {
    Duration::from_secs_f64(d.as_secs_f64() * factor.clamp(0.0, 1.0))
}

/// Full jitter: a random wait in `[0, min(max_delay, base_delay · 2^(attempt-1))]`.
/// `attempt` counts failures so far (1 after the first). `r` is uniform in `[0, 1)`.
pub(crate) fn backoff_delay(attempt: u32, policy: &RetryPolicy, r: f64) -> Duration {
    let exp = policy
        .base_delay
        .saturating_mul(1 << attempt.saturating_sub(1).min(30));
    scale(exp.min(policy.max_delay), r)
}

/// Wait before the next poll: grows from `min` towards `max` while there's no progress,
/// with full jitter above `min`. `step` is polls in a row without progress.
pub(crate) fn poll_delay(step: u32, min: Duration, max: Duration, r: f64) -> Duration {
    let cap = min.saturating_mul(1 << step.min(30)).min(max);
    min + scale(cap.saturating_sub(min), r)
}

/// Parses `Retry-After` (seconds or an HTTP date) relative to `now`.
pub(crate) fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

const TWO_POW_53: f64 = 9_007_199_254_740_992.0;

/// A small, fast PRNG for jitter (`SplitMix64`). Not for anything security related.
#[derive(Debug)]
pub(crate) struct Jitter(u64);

impl Jitter {
    pub(crate) fn seeded() -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        Self(u64::from(nanos) ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // The top 53 bits fit an f64 mantissa exactly, so these casts are lossless.
        #[allow(clippy::cast_precision_loss)]
        let x = (z >> 11) as f64;
        x / TWO_POW_53
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn grows_exponentially_then_caps() {
        let p = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(300),
        };
        let ms = |a| backoff_delay(a, &p, 0.999_999).as_millis();
        assert_eq!([ms(1), ms(2), ms(3), ms(40)], [99, 199, 299, 299]);
        assert_eq!(
            poll_delay(0, Duration::from_secs(2), Duration::from_secs(15), 0.5),
            Duration::from_secs(2)
        );
        assert_eq!(
            poll_delay(1, Duration::from_secs(1), Duration::from_secs(8), 0.5),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn parses_retry_after_seconds_and_dates() {
        let now = httpdate::parse_http_date("Thu, 01 Jan 2026 00:00:00 GMT").unwrap();
        assert_eq!(parse_retry_after(" 3 ", now), Some(Duration::from_secs(3)));
        assert_eq!(
            parse_retry_after("Thu, 01 Jan 2026 00:00:05 GMT", now),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            parse_retry_after("Wed, 31 Dec 2025 00:00:00 GMT", now),
            Some(Duration::ZERO)
        );
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[test]
    fn retries_only_transient_statuses() {
        assert!(
            [408, 425, 429, 500, 502, 503, 504]
                .into_iter()
                .all(is_retryable_status)
        );
        assert!(
            ![400, 401, 403, 404, 422, 501]
                .into_iter()
                .any(is_retryable_status)
        );
        assert_eq!(RetryPolicy::default().max_attempts, 3);
    }

    #[test]
    fn jitter_is_in_unit_range() {
        let mut j = Jitter::seeded();
        assert!(
            (0..1000)
                .map(|_| j.next_f64())
                .all(|x| (0.0..1.0).contains(&x))
        );
    }

    proptest! {
        #[test]
        fn nfr_3_backoff_never_exceeds_max_delay(attempt in 1u32..200, base in 0u64..5_000, extra in 0u64..60_000, r in 0.0f64..1.0) {
            let p = RetryPolicy { max_attempts: 3, base_delay: Duration::from_millis(base), max_delay: Duration::from_millis(base + extra) };
            prop_assert!(backoff_delay(attempt, &p, r) <= p.max_delay);
        }

        #[test]
        fn poll_delay_stays_in_range(step in 0u32..64, min in 1u64..5_000, extra in 0u64..60_000, r in 0.0f64..1.0) {
            let (lo, hi) = (Duration::from_millis(min), Duration::from_millis(min + extra));
            let d = poll_delay(step, lo, hi, r);
            prop_assert!(d >= lo && d <= hi);
        }
    }
}
