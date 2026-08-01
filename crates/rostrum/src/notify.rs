//! Desktop notifications for pull requests that appear between refreshes.
//!
//! The diff itself is pure: [`Baseline`] remembers the pull request numbers
//! each repository held on the previous observation and reports the ones that
//! were not there before. Posting is deliberately fire-and-forget on a
//! background thread — headless machines and CI have no notification daemon,
//! and a missing D-Bus service must never stall or crash the UI.

use std::collections::{HashMap, HashSet};

use gpui::{App, Context, Entity, Subscription};
use rostrum_core::{LoadState, PrNumber, RepoId, RepoState};

use crate::sync::Store;

/// A pull request that was not present on the previous observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPr {
    pub repo: RepoId,
    pub number: PrNumber,
    pub title: String,
}

impl NewPr {
    fn summary(&self) -> String {
        format!("New PR in {}", self.repo)
    }

    fn body(&self) -> String {
        format!("#{} {}", self.number.0, self.title)
    }
}

/// Pull request numbers seen per repository on the previous observation.
///
/// A repository absent from the map has no baseline yet, which is the whole
/// point: the first successful load of a repo populates it from empty, and
/// treating that as thirty arrivals would fire thirty notifications at startup.
#[derive(Debug, Default)]
pub struct Baseline {
    seen: HashMap<RepoId, HashSet<PrNumber>>,
}

impl Baseline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold the current repository states in, returning the pull requests that
    /// newly arrived.
    ///
    /// Only repositories in [`LoadState::Loaded`] are considered: a repo that
    /// is still loading, or whose refresh failed, has no authoritative list,
    /// and folding a partial one in would either miss arrivals or invent them.
    pub fn observe(&mut self, repos: &[RepoState]) -> Vec<NewPr> {
        let mut arrivals = Vec::new();

        for repo in repos {
            if !matches!(repo.load, LoadState::Loaded { .. }) {
                continue;
            }

            let current: HashSet<PrNumber> = repo.prs.iter().map(|pr| pr.number).collect();
            let new = newly_arrived(self.seen.get(&repo.id), &current);

            arrivals.extend(
                repo.prs
                    .iter()
                    .filter(|pr| new.contains(&pr.number))
                    .map(|pr| NewPr {
                        repo: repo.id.clone(),
                        number: pr.number,
                        title: pr.title.clone(),
                    }),
            );

            self.seen.insert(repo.id.clone(), current);
        }

        arrivals
    }
}

/// Numbers in `current` that were not in `previous`.
///
/// `previous` of `None` means "no baseline yet", which yields nothing: the
/// first observation establishes the baseline instead of announcing it.
/// Numbers that disappeared are simply forgotten, never reported.
fn newly_arrived(
    previous: Option<&HashSet<PrNumber>>,
    current: &HashSet<PrNumber>,
) -> HashSet<PrNumber> {
    let Some(previous) = previous else {
        return HashSet::new();
    };
    current.difference(previous).copied().collect()
}

/// Watches the store and posts a desktop notification per newly arrived pull
/// request. Held by the workspace; dropping it stops the notifications.
pub struct Notifier {
    baseline: Baseline,
    enabled: bool,
    _subscription: Subscription,
}

impl Notifier {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let enabled = store.read(cx).config.notifications;
        let mut baseline = Baseline::new();
        // Seed from whatever has already loaded so a notifier created mid-flight
        // does not announce the existing feed.
        baseline.observe(&store.read(cx).state.repos);

        let subscription = cx.observe(&store, |this: &mut Self, store, cx| {
            let arrivals = this.baseline.observe(&store.read(cx).state.repos);
            if this.enabled {
                for arrival in arrivals {
                    post(arrival, cx);
                }
            }
        });

        tracing::debug!(enabled, "notifier started");

        Self {
            baseline,
            enabled,
            _subscription: subscription,
        }
    }
}

/// Post one notification off the UI thread.
///
/// `notify-rust` talks to the platform notification service synchronously, so
/// this runs on the background executor and swallows every failure into a
/// `warn`: no daemon is a normal state, not an error worth surfacing.
fn post(new: NewPr, cx: &mut App) {
    let summary = new.summary();
    let body = new.body();

    cx.background_executor()
        .spawn(async move {
            let result = notify_rust::Notification::new()
                .appname("rostrum")
                .summary(&summary)
                .body(&body)
                .show();

            match result {
                Ok(handle) => drop(handle),
                Err(err) => tracing::warn!(error = %err, "could not post desktop notification"),
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rostrum_core::{MergeStateStatus, Mergeable, PullRequest};

    fn numbers(values: &[u32]) -> HashSet<PrNumber> {
        values.iter().copied().map(PrNumber).collect()
    }

    fn pr(number: u32) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: "feature".into(),
            head_sha: "deadbeef".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            merge_state: MergeStateStatus::Unknown,
            review_decision: None,
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
        }
    }

    fn loaded(name: &str, values: &[u32]) -> RepoState {
        RepoState {
            id: name.parse().expect("valid repo id"),
            prs: values.iter().copied().map(pr).collect(),
            load: LoadState::Loaded { at: Utc::now() },
            collapsed: false,
        }
    }

    // --- newly_arrived -----------------------------------------------------

    #[test]
    fn first_observation_reports_nothing() {
        assert!(newly_arrived(None, &numbers(&[1, 2, 3])).is_empty());
    }

    #[test]
    fn unchanged_set_reports_nothing() {
        let previous = numbers(&[1, 2, 3]);
        assert!(newly_arrived(Some(&previous), &numbers(&[1, 2, 3])).is_empty());
    }

    #[test]
    fn additions_are_reported() {
        let previous = numbers(&[1, 2]);
        assert_eq!(
            newly_arrived(Some(&previous), &numbers(&[1, 2, 3, 4])),
            numbers(&[3, 4])
        );
    }

    #[test]
    fn removals_are_not_reported() {
        let previous = numbers(&[1, 2, 3]);
        assert!(newly_arrived(Some(&previous), &numbers(&[1, 3])).is_empty());
    }

    /// A merged PR going away and an unrelated one arriving in the same refresh
    /// must report only the arrival.
    #[test]
    fn simultaneous_addition_and_removal_reports_only_the_addition() {
        let previous = numbers(&[1, 2]);
        assert_eq!(
            newly_arrived(Some(&previous), &numbers(&[2, 3])),
            numbers(&[3])
        );
    }

    #[test]
    fn an_empty_baseline_still_counts_as_established() {
        let previous = numbers(&[]);
        assert_eq!(
            newly_arrived(Some(&previous), &numbers(&[7])),
            numbers(&[7])
        );
    }

    // --- Baseline ----------------------------------------------------------

    #[test]
    fn baseline_stays_silent_on_first_load_then_reports_arrivals() {
        let mut baseline = Baseline::new();

        assert!(baseline.observe(&[loaded("a/b", &[1, 2])]).is_empty());
        assert!(baseline.observe(&[loaded("a/b", &[1, 2])]).is_empty());

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2, 5])]);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(5));
        assert_eq!(arrivals[0].repo.to_string(), "a/b");
        assert_eq!(arrivals[0].title, "PR 5");
    }

    #[test]
    fn baselines_are_tracked_per_repository() {
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1])]).is_empty());

        // c/d appears for the first time in the same pass that a/b gains a PR:
        // only a/b's arrival is announced.
        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2]), loaded("c/d", &[9, 10])]);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].repo.to_string(), "a/b");
        assert_eq!(arrivals[0].number, PrNumber(2));
    }

    /// Only a completed fetch is authoritative. Loading and failed repos must
    /// not establish a baseline, or the first successful load would announce
    /// every pull request it returns.
    #[test]
    fn unloaded_repos_do_not_establish_a_baseline() {
        let mut baseline = Baseline::new();

        let mut loading = loaded("a/b", &[]);
        loading.load = LoadState::Loading;
        let mut failed = loaded("c/d", &[]);
        failed.load = LoadState::Failed {
            message: "boom".into(),
            at: Utc::now(),
        };

        assert!(baseline.observe(&[loading, failed]).is_empty());

        // The first *completed* load is still the baseline, not three arrivals.
        assert!(baseline.observe(&[loaded("a/b", &[1, 2, 3])]).is_empty());
        assert!(baseline.observe(&[loaded("a/b", &[1, 2, 3])]).is_empty());

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2, 3, 4])]);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(4));
    }

    /// A PR that disappears and later comes back is a genuine arrival again;
    /// what must not happen is the removal itself being announced.
    #[test]
    fn a_returning_pr_is_reported_again() {
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1, 2])]).is_empty());
        assert!(baseline.observe(&[loaded("a/b", &[1])]).is_empty());

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2])]);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(2));
    }

    #[test]
    fn notification_text_names_the_repo_and_the_pr() {
        let new = NewPr {
            repo: "owner/repo".parse().expect("valid repo id"),
            number: PrNumber(123),
            title: "Title".into(),
        };
        assert_eq!(new.summary(), "New PR in owner/repo");
        assert_eq!(new.body(), "#123 Title");
    }
}
