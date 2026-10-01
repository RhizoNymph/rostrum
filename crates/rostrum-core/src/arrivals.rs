//! What is new since the last look: pull requests that appeared, and review
//! requests that arrived.
//!
//! The diff is pure. [`Baseline`] remembers, per repository, which pull
//! requests were open and which were waiting on the viewer's review at the
//! previous observation, and reports the differences. It serialises, so a
//! background job that runs once in a while can keep it between runs; the
//! desktop keeps it in memory for a session.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    model::{LoginKey, PrNumber, PullRequest, RepoId, User},
    state::{LoadState, RepoState},
};

/// A pull request that newly matters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrival {
    pub repo: RepoId,
    pub number: PrNumber,
    pub title: String,
    pub url: String,
    pub author: Option<User>,
    pub kind: ArrivalKind,
}

/// Why a pull request is being reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArrivalKind {
    /// It was not open at the previous observation.
    Opened,
    /// The viewer's review is requested and was not at the previous
    /// observation.
    ReviewRequested,
}

/// Pull requests seen per repository on the previous observation.
///
/// A repository absent from the baseline has none yet, which is the whole
/// point: its first successful load populates it, and treating that as thirty
/// arrivals would fire thirty notifications. The same holds for review
/// requests, which cannot be baselined until the viewer is known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Keyed by `owner/name`: a JSON object key has to be a string.
    repos: BTreeMap<String, RepoBaseline>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct RepoBaseline {
    open: BTreeSet<PrNumber>,
    /// `None` until an observation was made with the viewer known.
    #[serde(default)]
    review_requested: Option<BTreeSet<PrNumber>>,
}

impl Baseline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether nothing has been observed yet.
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
    }

    /// Fold the current repository states in, returning what newly arrived.
    ///
    /// - Only repositories in [`LoadState::Loaded`] are considered: one that is
    ///   loading, or whose refresh failed, has no authoritative list, and
    ///   folding a partial one in would either miss arrivals or invent them.
    /// - Numbers that disappeared are forgotten, never reported, so a pull
    ///   request that closes and reopens is an arrival again.
    /// - With `viewer` unknown, review requests are neither reported nor
    ///   re-baselined; the previous set stands until the viewer is known.
    /// - Repositories no longer in `repos` are dropped, so one removed and
    ///   re-added months later starts from a fresh baseline instead of
    ///   announcing everything opened in between.
    ///
    /// Arrivals come out in repository order, then feed order within each;
    /// a pull request can arrive as both kinds at once.
    pub fn observe(&mut self, repos: &[RepoState], viewer: Option<&LoginKey>) -> Vec<Arrival> {
        let watched: BTreeSet<String> = repos.iter().map(|repo| repo.id.to_string()).collect();
        self.repos.retain(|id, _| watched.contains(id));

        let mut arrivals = Vec::new();
        for repo in repos {
            if !matches!(repo.load, LoadState::Loaded { .. }) {
                continue;
            }

            let open: BTreeSet<PrNumber> = repo.prs.iter().map(|pr| pr.number).collect();
            let requested: Option<BTreeSet<PrNumber>> = viewer.map(|viewer| {
                repo.prs
                    .iter()
                    .filter(|pr| is_review_requested_from(pr, viewer))
                    .map(|pr| pr.number)
                    .collect()
            });

            let key = repo.id.to_string();
            let Some(previous) = self.repos.get(&key) else {
                self.repos.insert(
                    key,
                    RepoBaseline {
                        open,
                        review_requested: requested,
                    },
                );
                continue;
            };

            let opened = newly_arrived(Some(&previous.open), &open);
            let newly_requested = match (&requested, &previous.review_requested) {
                (Some(now), Some(before)) => newly_arrived(Some(before), now),
                _ => BTreeSet::new(),
            };

            for pr in &repo.prs {
                if opened.contains(&pr.number) {
                    arrivals.push(arrival(&repo.id, pr, ArrivalKind::Opened));
                }
                if newly_requested.contains(&pr.number) {
                    arrivals.push(arrival(&repo.id, pr, ArrivalKind::ReviewRequested));
                }
            }

            let review_requested = requested.or_else(|| previous.review_requested.clone());
            self.repos.insert(
                key,
                RepoBaseline {
                    open,
                    review_requested,
                },
            );
        }

        arrivals
    }
}

/// Whether `viewer` has an outstanding review request on `pr`.
pub fn is_review_requested_from(pr: &PullRequest, viewer: &LoginKey) -> bool {
    pr.review_requests.iter().any(|user| &user.key() == viewer)
}

fn arrival(repo: &RepoId, pr: &PullRequest, kind: ArrivalKind) -> Arrival {
    Arrival {
        repo: repo.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        author: pr.author.clone(),
        kind,
    }
}

/// Numbers in `current` that were not in `previous`.
///
/// `previous` of `None` means "no baseline yet", which yields nothing: the
/// first observation establishes the baseline instead of announcing it.
/// Numbers that disappeared are simply forgotten, never reported.
pub fn newly_arrived(
    previous: Option<&BTreeSet<PrNumber>>,
    current: &BTreeSet<PrNumber>,
) -> BTreeSet<PrNumber> {
    let Some(previous) = previous else {
        return BTreeSet::new();
    };
    current.difference(previous).copied().collect()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::test_support::pull;

    fn numbers(values: &[u32]) -> BTreeSet<PrNumber> {
        values.iter().copied().map(PrNumber).collect()
    }

    fn loaded(name: &str, values: &[u32]) -> RepoState {
        RepoState {
            id: name.parse().expect("valid repo id"),
            prs: values.iter().copied().map(pull).collect(),
            load: LoadState::Loaded { at: Utc::now() },
            collapsed: false,
            meta: None,
        }
    }

    fn requesting(mut repo: RepoState, login: &str, values: &[u32]) -> RepoState {
        for pr in &mut repo.prs {
            if values.contains(&pr.number.0) {
                pr.review_requests.push(User {
                    login: login.into(),
                    avatar_url: None,
                });
            }
        }
        repo
    }

    fn kinds(arrivals: &[Arrival]) -> Vec<(u32, ArrivalKind)> {
        arrivals.iter().map(|a| (a.number.0, a.kind)).collect()
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

    // --- Baseline: new pull requests ----------------------------------------

    #[test]
    fn baseline_stays_silent_on_first_load_then_reports_arrivals() {
        let mut baseline = Baseline::new();

        assert!(baseline.observe(&[loaded("a/b", &[1, 2])], None).is_empty());
        assert!(baseline.observe(&[loaded("a/b", &[1, 2])], None).is_empty());

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2, 5])], None);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(5));
        assert_eq!(arrivals[0].repo.to_string(), "a/b");
        assert_eq!(arrivals[0].title, "PR 5");
        assert_eq!(arrivals[0].kind, ArrivalKind::Opened);
    }

    #[test]
    fn baselines_are_tracked_per_repository() {
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1])], None).is_empty());

        // c/d appears for the first time in the same pass that a/b gains a PR:
        // only a/b's arrival is announced.
        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2]), loaded("c/d", &[9, 10])], None);
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

        assert!(baseline.observe(&[loading, failed], None).is_empty());
        assert!(baseline.is_empty());

        // The first *completed* load is still the baseline, not three arrivals.
        assert!(
            baseline
                .observe(&[loaded("a/b", &[1, 2, 3])], None)
                .is_empty()
        );
        assert!(
            baseline
                .observe(&[loaded("a/b", &[1, 2, 3])], None)
                .is_empty()
        );

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2, 3, 4])], None);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(4));
    }

    /// A failed refresh keeps the previous baseline rather than erasing it.
    #[test]
    fn a_failed_refresh_keeps_the_previous_baseline() {
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1])], None).is_empty());

        let mut failed = loaded("a/b", &[]);
        failed.load = LoadState::Failed {
            message: "offline".into(),
            at: Utc::now(),
        };
        assert!(baseline.observe(&[failed], None).is_empty());

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2])], None);
        assert_eq!(kinds(&arrivals), vec![(2, ArrivalKind::Opened)]);
    }

    /// A PR that disappears and later comes back is a genuine arrival again;
    /// what must not happen is the removal itself being announced.
    #[test]
    fn a_returning_pr_is_reported_again() {
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1, 2])], None).is_empty());
        assert!(baseline.observe(&[loaded("a/b", &[1])], None).is_empty());

        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2])], None);
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(2));
    }

    #[test]
    fn a_removed_repository_starts_over_when_watched_again() {
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1])], None).is_empty());
        // a/b is no longer watched.
        assert!(baseline.observe(&[loaded("c/d", &[5])], None).is_empty());
        // Re-added with far more open: a fresh baseline, not three arrivals.
        let arrivals = baseline.observe(&[loaded("a/b", &[1, 2, 3, 4]), loaded("c/d", &[5])], None);
        assert!(arrivals.is_empty(), "{arrivals:?}");
    }

    // --- Baseline: review requests -------------------------------------------

    #[test]
    fn review_requests_are_baselined_then_reported_as_they_arrive() {
        let me = LoginKey::new("Me");
        let mut baseline = Baseline::new();

        let first = requesting(loaded("a/b", &[1, 2]), "me", &[1]);
        assert!(baseline.observe(&[first], Some(&me)).is_empty());

        // #2 now asks for my review; #3 is new and asks too.
        let second = requesting(loaded("a/b", &[1, 2, 3]), "ME", &[1, 2, 3]);
        let arrivals = baseline.observe(std::slice::from_ref(&second), Some(&me));
        assert_eq!(
            kinds(&arrivals),
            vec![
                (2, ArrivalKind::ReviewRequested),
                (3, ArrivalKind::Opened),
                (3, ArrivalKind::ReviewRequested),
            ]
        );

        // Nothing changed: nothing reported.
        assert!(baseline.observe(&[second], Some(&me)).is_empty());
    }

    #[test]
    fn someone_elses_review_request_is_not_mine() {
        let me = LoginKey::new("me");
        let mut baseline = Baseline::new();
        assert!(
            baseline
                .observe(&[loaded("a/b", &[1])], Some(&me))
                .is_empty()
        );
        let other = requesting(loaded("a/b", &[1]), "someone", &[1]);
        assert!(baseline.observe(&[other], Some(&me)).is_empty());
    }

    /// Without a viewer the request set cannot be computed, so it is neither
    /// reported nor overwritten: the next observation with a viewer compares
    /// against the last one that had one.
    #[test]
    fn an_unknown_viewer_neither_reports_nor_forgets_review_requests() {
        let me = LoginKey::new("me");
        let mut baseline = Baseline::new();
        assert!(
            baseline
                .observe(&[requesting(loaded("a/b", &[1, 2]), "me", &[1])], Some(&me))
                .is_empty()
        );

        let asked = requesting(loaded("a/b", &[1, 2]), "me", &[1, 2]);
        assert!(
            baseline
                .observe(std::slice::from_ref(&asked), None)
                .is_empty()
        );

        let arrivals = baseline.observe(&[asked], Some(&me));
        assert_eq!(kinds(&arrivals), vec![(2, ArrivalKind::ReviewRequested)]);
    }

    /// A baseline written with no viewer establishes the request set only once
    /// a viewer is known, rather than reporting every open request then.
    #[test]
    fn review_requests_baseline_once_the_viewer_is_first_known() {
        let me = LoginKey::new("me");
        let mut baseline = Baseline::new();
        assert!(baseline.observe(&[loaded("a/b", &[1])], None).is_empty());

        let asked = requesting(loaded("a/b", &[1]), "me", &[1]);
        assert!(
            baseline
                .observe(std::slice::from_ref(&asked), Some(&me))
                .is_empty()
        );
        assert!(baseline.observe(&[asked], Some(&me)).is_empty());
    }

    #[test]
    fn a_baseline_survives_serialisation() {
        let me = LoginKey::new("me");
        let mut baseline = Baseline::new();
        baseline.observe(
            &[
                requesting(loaded("a/b", &[1, 2]), "me", &[2]),
                loaded("c/d", &[7]),
            ],
            Some(&me),
        );

        let json = serde_json::to_string(&baseline).expect("serialises");
        let mut restored: Baseline = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(restored, baseline);

        let arrivals = restored.observe(
            &[
                requesting(loaded("a/b", &[1, 2, 3]), "me", &[2]),
                loaded("c/d", &[7]),
            ],
            Some(&me),
        );
        assert_eq!(kinds(&arrivals), vec![(3, ArrivalKind::Opened)]);
    }
}
