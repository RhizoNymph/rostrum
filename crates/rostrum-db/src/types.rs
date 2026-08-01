//! Values handed back across the crate boundary.

use chrono::{DateTime, SecondsFormat, Utc};
use rostrum_github::DraftComment;

use crate::error::DbError;

/// A cached HTTP response body together with the ETag it arrived with, so the
/// next request for the same URL can be made conditional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedResponse {
    pub etag: String,
    pub body: String,
}

/// The locally authored, never-sent review comments for one pull request.
///
/// `head_sha` is the commit the comments were anchored against. A caller that
/// finds it no longer matches the pull request's head knows the line anchors
/// may have moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftSet {
    pub head_sha: String,
    pub comments: Vec<DraftComment>,
    pub updated_at: DateTime<Utc>,
}

/// Fixed-width RFC 3339 in UTC, which also makes the text column sort
/// chronologically — that is what lets `prune_cache` compare with `<`.
pub(crate) fn encode_time(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub(crate) fn decode_time(value: &str) -> Result<DateTime<Utc>, DbError> {
    DateTime::parse_from_rfc3339(value)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|source| DbError::Timestamp {
            value: value.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_round_trip() {
        let at = DateTime::from_timestamp(1_700_000_000, 123_456_000).expect("valid timestamp");
        assert_eq!(decode_time(&encode_time(at)).expect("decodes"), at);
    }

    #[test]
    fn encoded_timestamps_sort_chronologically() {
        let early = encode_time(DateTime::from_timestamp(1_000, 0).expect("valid"));
        let late = encode_time(DateTime::from_timestamp(1_700_000_000, 0).expect("valid"));
        assert!(early < late);
    }

    #[test]
    fn malformed_timestamp_is_an_error() {
        assert!(matches!(
            decode_time("not-a-time"),
            Err(DbError::Timestamp { .. })
        ));
    }
}
