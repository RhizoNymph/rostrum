//! When a repository's CI checks must be fetched again, and when the last
//! answer still stands.
//!
//! Checks hang off a pull request's head commit, so while every head is
//! where it was and nothing was still running, the last answer is the
//! answer. Drafts are left out on both sides: the CI view never shows them.

use std::{collections::BTreeSet, time::Duration};

use chrono::{DateTime, Utc};

use super::{Trigger, older_than};
use crate::{
    ci::{CiChecks, RepoChecks},
    model::{PrNumber, PullRequest, RepoId},
    state::{LoadState, RepoState},
};

/// Why a repository's checks are fetched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiReason {
    /// The user asked for a refresh.
    Explicit,
    /// Never fetched.
    NeverFetched,
    /// The last fetch failed.
    LastFailed,
    /// A pull request the last answer did not cover.
    PrAdded(PrNumber),
    /// A pull request's head moved since its checks were fetched.
    HeadMoved(PrNumber),
    /// Something was queued or running.
    Running,
    /// Unchanged, but older than the safety interval.
    SafetyNet,
}

impl CiReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::NeverFetched => "never fetched",
            Self::LastFailed => "last failed",
            Self::PrAdded(_) => "pull request added",
            Self::HeadMoved(_) => "head moved",
            Self::Running => "running",
            Self::SafetyNet => "safety net",
        }
    }
}

/// Whether to fetch a repository's checks or keep the ones held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiDue {
    Reuse,
    /// A first fetch is already in flight.
    InFlight,
    Fetch(CiReason),
}

/// Judge one repository: `prs` are its open pull requests as the feed just
/// fetched them, `checks` what the last checks fetch left.
pub fn ci_due(
    prs: &[PullRequest],
    checks: Option<&RepoChecks>,
    now: DateTime<Utc>,
    safety: Duration,
    trigger: Trigger,
) -> CiDue {
    let fetched_at = match checks.map(|checks| &checks.load) {
        None | Some(LoadState::Idle) => {
            return CiDue::Fetch(if trigger == Trigger::Explicit {
                CiReason::Explicit
            } else {
                CiReason::NeverFetched
            });
        }
        Some(LoadState::Loading) => return CiDue::InFlight,
        Some(LoadState::Failed { .. }) => None,
        Some(LoadState::Loaded { at }) => Some(*at),
    };
    if trigger == Trigger::Explicit {
        return CiDue::Fetch(CiReason::Explicit);
    }
    let Some(fetched_at) = fetched_at else {
        return CiDue::Fetch(CiReason::LastFailed);
    };
    let Some(checks) = checks else {
        return CiDue::Fetch(CiReason::NeverFetched);
    };

    let mut running = false;
    for pr in prs.iter().filter(|pr| !pr.is_draft) {
        let Some(held) = checks.prs.get(&pr.number) else {
            return CiDue::Fetch(CiReason::PrAdded(pr.number));
        };
        if held.head_sha != pr.head_sha {
            return CiDue::Fetch(CiReason::HeadMoved(pr.number));
        }
        running |= held
            .latest()
            .values()
            .any(|entry| entry.status.is_running());
    }
    if running {
        return CiDue::Fetch(CiReason::Running);
    }
    if older_than(fetched_at, now, safety) {
        return CiDue::Fetch(CiReason::SafetyNet);
    }
    CiDue::Reuse
}

/// [`ci_due`] over every repository whose pull requests have loaded (or
/// only those in `only`), in feed order: the ones to fetch, with why.
pub fn plan_ci(
    repos: &[RepoState],
    checks: &CiChecks,
    now: DateTime<Utc>,
    safety: Duration,
    trigger: Trigger,
    only: Option<&BTreeSet<RepoId>>,
) -> Vec<(RepoId, CiReason)> {
    repos
        .iter()
        .filter(|repo| only.is_none_or(|only| only.contains(&repo.id)))
        .filter(|repo| matches!(repo.load, LoadState::Loaded { .. }))
        .filter_map(
            |repo| match ci_due(&repo.prs, checks.repo(&repo.id), now, safety, trigger) {
                CiDue::Fetch(reason) => Some((repo.id.clone(), reason)),
                CiDue::Reuse | CiDue::InFlight => None,
            },
        )
        .collect()
}

/// Repositories with anything queued or running: what the CI view's faster
/// watch re-checks.
pub fn running_repos(repos: &[RepoState], checks: &CiChecks) -> Vec<RepoId> {
    repos
        .iter()
        .filter(|repo| {
            checks.repo(&repo.id).is_some_and(|held| {
                held.prs
                    .values()
                    .any(|pr| pr.latest().values().any(|entry| entry.status.is_running()))
            })
        })
        .map(|repo| repo.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ci::{CheckEntry, CheckKey, CheckSource, CheckStatus, PrChecks},
        test_support::pull,
    };

    const SAFETY: Duration = Duration::from_secs(600);

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    fn id(name: &str) -> RepoId {
        name.parse().expect("valid repo id")
    }

    fn pr(number: u32, head: &str) -> PullRequest {
        PullRequest {
            head_sha: head.into(),
            ..pull(number)
        }
    }

    fn draft(number: u32, head: &str) -> PullRequest {
        PullRequest {
            is_draft: true,
            ..pr(number, head)
        }
    }

    fn checks_for(number: u32, head: &str, status: CheckStatus) -> PrChecks {
        PrChecks {
            number: PrNumber(number),
            head_sha: head.into(),
            entries: vec![CheckEntry {
                key: CheckKey::new(Some("CI".into()), "test"),
                status,
                started_at: None,
                completed_at: None,
                details_url: None,
                source: CheckSource::Status,
            }],
            truncated: false,
        }
    }

    fn loaded(prs: Vec<PrChecks>, fetched: i64) -> RepoChecks {
        RepoChecks {
            prs: prs.into_iter().map(|pr| (pr.number, pr)).collect(),
            load: LoadState::Loaded { at: at(fetched) },
        }
    }

    fn judge(prs: &[PullRequest], checks: Option<&RepoChecks>, now: i64) -> CiDue {
        ci_due(prs, checks, at(now), SAFETY, Trigger::Timer)
    }

    #[test]
    fn nothing_changed_reuses() {
        let held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        assert_eq!(judge(&[pr(1, "aaa")], Some(&held), 60), CiDue::Reuse);
    }

    #[test]
    fn never_fetched_is_fetched() {
        assert_eq!(
            judge(&[pr(1, "aaa")], None, 0),
            CiDue::Fetch(CiReason::NeverFetched)
        );
        let idle = RepoChecks::default();
        assert_eq!(
            judge(&[pr(1, "aaa")], Some(&idle), 0),
            CiDue::Fetch(CiReason::NeverFetched)
        );
    }

    #[test]
    fn a_first_fetch_in_flight_is_left_alone() {
        let loading = RepoChecks {
            load: LoadState::Loading,
            ..RepoChecks::default()
        };
        assert_eq!(judge(&[pr(1, "aaa")], Some(&loading), 0), CiDue::InFlight);
    }

    #[test]
    fn a_failed_fetch_is_retried() {
        let mut held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        held.load = LoadState::Failed {
            message: "boom".into(),
            at: at(30),
        };
        assert_eq!(
            judge(&[pr(1, "aaa")], Some(&held), 60),
            CiDue::Fetch(CiReason::LastFailed)
        );
    }

    #[test]
    fn a_moved_head_is_fetched() {
        let held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        assert_eq!(
            judge(&[pr(1, "bbb")], Some(&held), 60),
            CiDue::Fetch(CiReason::HeadMoved(PrNumber(1)))
        );
    }

    #[test]
    fn an_added_pull_request_is_fetched() {
        let held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        assert_eq!(
            judge(&[pr(1, "aaa"), pr(2, "ccc")], Some(&held), 60),
            CiDue::Fetch(CiReason::PrAdded(PrNumber(2)))
        );
    }

    #[test]
    fn a_closed_pull_request_changes_nothing_else() {
        let held = loaded(
            vec![
                checks_for(1, "aaa", CheckStatus::Success),
                checks_for(2, "ccc", CheckStatus::Success),
            ],
            0,
        );
        assert_eq!(judge(&[pr(1, "aaa")], Some(&held), 60), CiDue::Reuse);
    }

    #[test]
    fn anything_running_is_fetched() {
        for status in [CheckStatus::Queued, CheckStatus::InProgress] {
            let held = loaded(vec![checks_for(1, "aaa", status)], 0);
            assert_eq!(
                judge(&[pr(1, "aaa")], Some(&held), 60),
                CiDue::Fetch(CiReason::Running),
                "{status:?}"
            );
        }
    }

    #[test]
    fn the_safety_net_refetches_unchanged_checks() {
        let held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        assert_eq!(judge(&[pr(1, "aaa")], Some(&held), 599), CiDue::Reuse);
        assert_eq!(
            judge(&[pr(1, "aaa")], Some(&held), 600),
            CiDue::Fetch(CiReason::SafetyNet)
        );
    }

    #[test]
    fn drafts_never_make_a_fetch_due() {
        let held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        // A new draft, and a draft whose head moved with checks running.
        let prs = [pr(1, "aaa"), draft(2, "ddd")];
        assert_eq!(judge(&prs, Some(&held), 60), CiDue::Reuse);
        let held = loaded(
            vec![
                checks_for(1, "aaa", CheckStatus::Success),
                checks_for(2, "old", CheckStatus::InProgress),
            ],
            0,
        );
        assert_eq!(judge(&prs, Some(&held), 60), CiDue::Reuse);
    }

    #[test]
    fn an_explicit_refresh_fetches_even_unchanged_checks() {
        let held = loaded(vec![checks_for(1, "aaa", CheckStatus::Success)], 0);
        assert_eq!(
            ci_due(
                &[pr(1, "aaa")],
                Some(&held),
                at(1),
                SAFETY,
                Trigger::Explicit
            ),
            CiDue::Fetch(CiReason::Explicit)
        );
    }

    #[test]
    fn the_plan_covers_loaded_repositories_that_are_due() {
        let mut fresh = RepoState::new(id("a/fresh"));
        fresh.prs = vec![pr(1, "aaa")];
        fresh.load = LoadState::Loaded { at: at(0) };
        let mut moved = RepoState::new(id("a/moved"));
        moved.prs = vec![pr(1, "new")];
        moved.load = LoadState::Loaded { at: at(0) };
        let unloaded = RepoState::new(id("a/unloaded"));

        let mut checks = CiChecks::default();
        checks.loaded(
            &id("a/fresh"),
            vec![checks_for(1, "aaa", CheckStatus::Success)],
            at(0),
        );
        checks.loaded(
            &id("a/moved"),
            vec![checks_for(1, "old", CheckStatus::Success)],
            at(0),
        );

        let repos = [fresh, moved, unloaded];
        let plan = plan_ci(&repos, &checks, at(60), SAFETY, Trigger::Timer, None);
        assert_eq!(
            plan,
            vec![(id("a/moved"), CiReason::HeadMoved(PrNumber(1)))]
        );

        let only: BTreeSet<RepoId> = [id("a/fresh")].into();
        let plan = plan_ci(&repos, &checks, at(60), SAFETY, Trigger::Timer, Some(&only));
        assert!(plan.is_empty());
    }

    #[test]
    fn running_repositories_are_found() {
        let mut checks = CiChecks::default();
        checks.loaded(
            &id("a/busy"),
            vec![checks_for(1, "a", CheckStatus::InProgress)],
            at(0),
        );
        checks.loaded(
            &id("a/idle"),
            vec![checks_for(1, "a", CheckStatus::Success)],
            at(0),
        );
        let repos = [RepoState::new(id("a/busy")), RepoState::new(id("a/idle"))];
        assert_eq!(running_repos(&repos, &checks), vec![id("a/busy")]);
    }
}
