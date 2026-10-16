//! The client's view of GitHub's rate-limit budget: every response's
//! `x-ratelimit-*` headers, every rate-limit refusal, and every request's
//! cost, folded into one [`BudgetState`] that the poller reads.
//!
//! Shared by every clone of a [`GitHubClient`](super::GitHubClient) — the
//! store's, the detail panes', the CI view's — because they all spend the
//! same budget. The lock is held only to copy or fold plain data, never
//! across an await.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use chrono::{DateTime, TimeZone, Utc};
use reqwest::{StatusCode, header::HeaderMap};
use rostrum_core::budget::{
    BudgetState, Feature, Jitter, RateBudget, RateLimitHit, RateResource, SECONDARY_DEFAULT_RETRY,
    SpendTally,
};

/// Every clone's shared budget state.
#[derive(Clone, Debug, Default)]
pub struct BudgetTracker {
    state: Arc<Mutex<BudgetState>>,
}

impl BudgetTracker {
    /// A copy of everything known now.
    pub fn snapshot(&self) -> BudgetState {
        self.lock().clone()
    }

    /// Hand over what has been spent since the last call.
    pub fn take_spend(&self) -> SpendTally {
        self.lock().take_spend()
    }

    /// Fold in one response: its budget headers, and the pause a refusal
    /// calls for.
    pub(crate) fn record_response(&self, status: StatusCode, headers: &HeaderMap, body: &str) {
        let now = Utc::now();
        let observed = budget_from_headers(headers);
        let hit = rate_limit_hit(status, headers, body, now);
        if observed.is_none() && hit.is_none() {
            return;
        }
        let mut state = self.lock();
        if let Some((resource, budget)) = observed {
            state.observe(resource, budget);
        }
        if let Some(hit) = hit {
            tracing::warn!(
                ?hit,
                status = status.as_u16(),
                "GitHub rate limit hit; pausing polls"
            );
            state.hit(hit, now, jitter(now));
        }
    }

    /// A GraphQL answer refused for rate limiting inside a 200
    /// (`errors[].type == "RATE_LIMITED"`).
    pub(crate) fn record_hit(&self, hit: RateLimitHit) {
        let now = Utc::now();
        tracing::warn!(?hit, "GitHub rate limit hit; pausing polls");
        self.lock().hit(hit, now, jitter(now));
    }

    pub(crate) fn record_graphql(&self, feature: Feature, cost: u32) {
        self.lock().spend.record_graphql(feature, cost);
    }

    pub(crate) fn record_rest(&self, feature: Feature) {
        self.lock().spend.record_rest(feature);
    }

    fn lock(&self) -> MutexGuard<'_, BudgetState> {
        // The state is plain data that every writer leaves whole, so a
        // panic elsewhere while holding the lock cannot have broken it.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn jitter(now: DateTime<Utc>) -> Jitter {
    Jitter::spread(u64::from(now.timestamp_subsec_nanos()))
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    header(headers, name)?.trim().parse().ok()
}

fn reset_from_headers(headers: &HeaderMap) -> Option<DateTime<Utc>> {
    let secs = i64::try_from(header_u64(headers, "x-ratelimit-reset")?).ok()?;
    Utc.timestamp_opt(secs, 0).single()
}

/// The budget a response reports, if it names a resource this tracks.
pub(crate) fn budget_from_headers(headers: &HeaderMap) -> Option<(RateResource, RateBudget)> {
    let resource = RateResource::from_header(header(headers, "x-ratelimit-resource")?)?;
    let limit = u32::try_from(header_u64(headers, "x-ratelimit-limit")?).ok()?;
    let remaining = u32::try_from(header_u64(headers, "x-ratelimit-remaining")?).ok()?;
    let budget = RateBudget::new(limit, remaining, reset_from_headers(headers)?)?;
    Some((resource, budget))
}

/// Whether a response is GitHub refusing for rate limiting, and which kind.
///
/// Mirrors [`super::classify_status`]: 403 or 429 with `retry-after` or
/// GitHub's "secondary rate limit" wording is secondary; with
/// `x-ratelimit-remaining: 0` it is primary.
pub(crate) fn rate_limit_hit(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
    now: DateTime<Utc>,
) -> Option<RateLimitHit> {
    if !matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        return None;
    }
    if let Some(secs) = header_u64(headers, "retry-after") {
        return Some(RateLimitHit::Secondary {
            retry_after: Duration::from_secs(secs),
        });
    }
    if mentions_secondary_limit(body) {
        return Some(RateLimitHit::Secondary {
            retry_after: SECONDARY_DEFAULT_RETRY,
        });
    }
    if header_u64(headers, "x-ratelimit-remaining") == Some(0) {
        return Some(RateLimitHit::Primary {
            reset_at: reset_from_headers(headers).unwrap_or(now + chrono::Duration::seconds(60)),
        });
    }
    None
}

/// GitHub's wording for a secondary limit, which a 403 can carry without a
/// `retry-after` header.
pub(crate) fn mentions_secondary_limit(body: &str) -> bool {
    body.to_ascii_lowercase().contains("secondary rate limit")
}

/// The reset a GraphQL `RATE_LIMITED` answer implies: its headers' reset, or
/// a minute from now when they carry none.
pub(crate) fn primary_reset(headers: &HeaderMap, now: DateTime<Utc>) -> DateTime<Utc> {
    reset_from_headers(headers).unwrap_or(now + chrono::Duration::seconds(60))
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderName, HeaderValue};

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("valid header"),
                HeaderValue::from_str(value).expect("valid value"),
            );
        }
        map
    }

    fn graphql_headers(remaining: &str) -> HeaderMap {
        headers(&[
            ("x-ratelimit-resource", "graphql"),
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", remaining),
            ("x-ratelimit-used", "4"),
            ("x-ratelimit-reset", "1800000000"),
        ])
    }

    #[test]
    fn budget_headers_are_read() {
        let (resource, budget) = budget_from_headers(&graphql_headers("4996")).expect("budget");
        assert_eq!(resource, RateResource::Graphql);
        assert_eq!(budget.limit(), 5000);
        assert_eq!(budget.remaining(), 4996);
        assert_eq!(budget.reset_at().timestamp(), 1_800_000_000);
    }

    #[test]
    fn untracked_or_incomplete_headers_are_ignored() {
        assert!(budget_from_headers(&HeaderMap::new()).is_none());
        let search = headers(&[
            ("x-ratelimit-resource", "search"),
            ("x-ratelimit-limit", "30"),
            ("x-ratelimit-remaining", "29"),
            ("x-ratelimit-reset", "1800000000"),
        ]);
        assert!(budget_from_headers(&search).is_none());
        let no_reset = headers(&[
            ("x-ratelimit-resource", "core"),
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "1"),
        ]);
        assert!(budget_from_headers(&no_reset).is_none());
    }

    #[test]
    fn an_exhausted_403_is_a_primary_hit() {
        let hit = rate_limit_hit(StatusCode::FORBIDDEN, &graphql_headers("0"), "", Utc::now());
        assert_eq!(
            hit,
            Some(RateLimitHit::Primary {
                reset_at: Utc.timestamp_opt(1_800_000_000, 0).single().expect("valid")
            })
        );
    }

    #[test]
    fn retry_after_is_a_secondary_hit() {
        let hit = rate_limit_hit(
            StatusCode::TOO_MANY_REQUESTS,
            &headers(&[("retry-after", "120")]),
            "",
            Utc::now(),
        );
        assert_eq!(
            hit,
            Some(RateLimitHit::Secondary {
                retry_after: Duration::from_secs(120)
            })
        );
    }

    #[test]
    fn secondary_wording_without_retry_after_waits_the_default() {
        let body = r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again."}"#;
        let hit = rate_limit_hit(
            StatusCode::FORBIDDEN,
            &graphql_headers("4000"),
            body,
            Utc::now(),
        );
        assert_eq!(
            hit,
            Some(RateLimitHit::Secondary {
                retry_after: SECONDARY_DEFAULT_RETRY
            })
        );
    }

    #[test]
    fn other_refusals_are_not_rate_limits() {
        let now = Utc::now();
        assert!(
            rate_limit_hit(StatusCode::FORBIDDEN, &graphql_headers("4000"), "SAML", now).is_none()
        );
        assert!(rate_limit_hit(StatusCode::OK, &graphql_headers("0"), "", now).is_none());
        assert!(rate_limit_hit(StatusCode::NOT_FOUND, &HeaderMap::new(), "", now).is_none());
    }

    #[test]
    fn the_tracker_folds_responses_and_pauses_on_a_hit() {
        let tracker = BudgetTracker::default();
        tracker.record_response(StatusCode::OK, &graphql_headers("4000"), "");
        let state = tracker.snapshot();
        assert_eq!(state.budgets.graphql.map(|b| b.remaining()), Some(4000));
        assert!(state.active_pause(Utc::now()).is_none());

        tracker.record_response(
            StatusCode::FORBIDDEN,
            &headers(&[("retry-after", "60")]),
            "",
        );
        assert!(tracker.snapshot().active_pause(Utc::now()).is_some());
    }

    #[test]
    fn clones_share_one_tracker() {
        let tracker = BudgetTracker::default();
        let clone = tracker.clone();
        clone.record_graphql(Feature::Ci, 3);
        clone.record_rest(Feature::Stacks);
        let spent = tracker.take_spend();
        assert_eq!(spent.points(Feature::Ci), 3);
        assert_eq!(spent.rest_requests(Feature::Stacks), 1);
        assert!(clone.take_spend().is_empty());
    }
}
