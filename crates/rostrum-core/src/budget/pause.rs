//! Stopping every poll while GitHub refuses for rate limiting.

use std::time::Duration;

use chrono::{DateTime, Utc};

/// How long to wait after a secondary rate limit that named no
/// `retry-after`. GitHub's guidance is "at least one minute".
pub const SECONDARY_DEFAULT_RETRY: Duration = Duration::from_secs(60);

/// Bounds of the delay added on top of the moment a limit lifts. The phone
/// and `gh` share the budget and the same reset instant; spreading the resume
/// keeps every client from firing in the same second.
pub const MIN_RESUME_JITTER: Duration = Duration::from_secs(5);
pub const MAX_RESUME_JITTER: Duration = Duration::from_secs(30);

/// GitHub refused a request for rate limiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitHit {
    /// The hourly budget is spent (403/429 with `x-ratelimit-remaining: 0`,
    /// or a GraphQL `RATE_LIMITED` error); it refills at `reset_at`.
    Primary { reset_at: DateTime<Utc> },
    /// Too many requests too quickly (a `retry-after` header, or GitHub's
    /// "secondary rate limit" wording).
    Secondary { retry_after: Duration },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseCause {
    Primary,
    Secondary,
}

/// The extra wait after a limit lifts, always within
/// [`MIN_RESUME_JITTER`]`..=`[`MAX_RESUME_JITTER`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jitter(Duration);

impl Jitter {
    /// `wait`, clamped into the allowed range.
    pub fn new(wait: Duration) -> Self {
        Self(wait.clamp(MIN_RESUME_JITTER, MAX_RESUME_JITTER))
    }

    /// Spread `seed` (anything that differs between clients — the clock's
    /// nanoseconds will do) across the allowed range.
    pub fn spread(seed: u64) -> Self {
        let span = (MAX_RESUME_JITTER - MIN_RESUME_JITTER).as_secs() + 1;
        Self(MIN_RESUME_JITTER + Duration::from_secs(seed % span))
    }

    pub fn duration(self) -> Duration {
        self.0
    }
}

/// Polling is stopped until `until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pause {
    until: DateTime<Utc>,
    cause: PauseCause,
}

impl Pause {
    /// The pause a refusal at `now` calls for.
    ///
    /// A primary limit waits for its reset. A reset that is already past
    /// means the clocks disagree, since GitHub has just refused; then it waits
    /// as long as a secondary limit would rather than hammering a budget that
    /// is evidently still spent.
    pub fn after(hit: RateLimitHit, now: DateTime<Utc>, jitter: Jitter) -> Self {
        let jitter = to_chrono(jitter.duration());
        match hit {
            RateLimitHit::Primary { reset_at } => {
                let lifts = if reset_at > now {
                    reset_at
                } else {
                    now + to_chrono(SECONDARY_DEFAULT_RETRY)
                };
                Self {
                    until: lifts + jitter,
                    cause: PauseCause::Primary,
                }
            }
            RateLimitHit::Secondary { retry_after } => Self {
                until: now + to_chrono(retry_after.max(Duration::from_secs(1))) + jitter,
                cause: PauseCause::Secondary,
            },
        }
    }

    pub fn until(&self) -> DateTime<Utc> {
        self.until
    }

    pub fn cause(&self) -> PauseCause {
        self.cause
    }

    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        now < self.until
    }

    /// How long from `now` until polling resumes; zero once it has.
    pub fn remaining(&self, now: DateTime<Utc>) -> Duration {
        self.until
            .signed_duration_since(now)
            .to_std()
            .unwrap_or(Duration::ZERO)
    }

    /// Whichever of the two pauses ends later.
    pub fn longer(self, other: Pause) -> Pause {
        if other.until > self.until {
            other
        } else {
            self
        }
    }
}

fn to_chrono(duration: Duration) -> chrono::Duration {
    chrono::Duration::from_std(duration).unwrap_or(chrono::Duration::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    #[test]
    fn jitter_stays_in_range() {
        assert_eq!(Jitter::new(Duration::ZERO).duration(), MIN_RESUME_JITTER);
        assert_eq!(
            Jitter::new(Duration::from_secs(3600)).duration(),
            MAX_RESUME_JITTER
        );
        for seed in [0, 1, 25, 26, 27, u64::MAX] {
            let wait = Jitter::spread(seed).duration();
            assert!(
                (MIN_RESUME_JITTER..=MAX_RESUME_JITTER).contains(&wait),
                "{seed}: {wait:?}"
            );
        }
        assert_ne!(Jitter::spread(1), Jitter::spread(2));
    }

    #[test]
    fn a_secondary_limit_waits_retry_after_plus_jitter() {
        let pause = Pause::after(
            RateLimitHit::Secondary {
                retry_after: Duration::from_secs(90),
            },
            at(0),
            Jitter::new(Duration::from_secs(7)),
        );
        assert_eq!(pause.until(), at(97));
        assert_eq!(pause.cause(), PauseCause::Secondary);
        assert_eq!(pause.remaining(at(90)), Duration::from_secs(7));
        assert_eq!(pause.remaining(at(200)), Duration::ZERO);
    }

    #[test]
    fn a_zero_retry_after_still_waits() {
        let pause = Pause::after(
            RateLimitHit::Secondary {
                retry_after: Duration::ZERO,
            },
            at(0),
            Jitter::new(MIN_RESUME_JITTER),
        );
        assert_eq!(pause.until(), at(6));
    }

    #[test]
    fn a_reset_already_past_waits_the_default() {
        let pause = Pause::after(
            RateLimitHit::Primary { reset_at: at(-30) },
            at(0),
            Jitter::new(MIN_RESUME_JITTER),
        );
        assert_eq!(pause.until(), at(65));
    }

    #[test]
    fn the_longer_pause_wins() {
        let jitter = Jitter::new(MIN_RESUME_JITTER);
        let short = Pause::after(RateLimitHit::Primary { reset_at: at(10) }, at(0), jitter);
        let long = Pause::after(RateLimitHit::Primary { reset_at: at(100) }, at(0), jitter);
        assert_eq!(short.longer(long), long);
        assert_eq!(long.longer(short), long);
    }
}
