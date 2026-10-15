//! Closed pull requests: the ones that were merged, and the ones closed
//! without merging, for the feed's Closed tab.
//!
//! A pull request that has left the open list ended in exactly one of two
//! ways, at one instant. [`PullEnding`] says which and when, so "merged and
//! closed at once" and "closed but no time" are not values a caller can hold.
//! GitHub reports the two as separate nullable fields beside a `state`;
//! [`PullEnding::from_parts`] is the one place those three are reconciled.
//!
//! The tab lists every watched repository's closed pull requests as one list,
//! newest ending first — merged ones by when they merged, the others by when
//! they closed. GitHub cannot order a connection by either time (only by
//! creation or last update), so the fetch takes the most recently *updated*
//! ones and [`closed_order`] sorts them here. See
//! `docs/features/closed_prs.md` for what that window can miss.

mod feed;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    authors::{AuthorEntry, roster_from},
    feed::{FeedFilter, RepoIx},
    model::{LoginKey, PrNumber, PullRequest, PullState, RepoId, User},
    state::{LoadState, RepoState},
};

pub(crate) use feed::flatten_closed;

/// How a pull request left the open list, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullEnding {
    /// Merged into its base at `at` (GitHub's `mergedAt`).
    Merged { at: DateTime<Utc> },
    /// Closed without merging at `at` (GitHub's `closedAt`).
    Closed { at: DateTime<Utc> },
}

/// Why GitHub's `state`, `mergedAt` and `closedAt` do not describe an ending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EndingError {
    #[error("the pull request is still open")]
    StillOpen,
    #[error("the pull request is merged but has no merge time")]
    MergedWithoutTime,
    #[error("the pull request is closed but has no close time")]
    ClosedWithoutTime,
}

impl PullEnding {
    /// Reconcile GitHub's three fields into one ending.
    ///
    /// `state` decides the kind; the matching time is then required. A merged
    /// pull request also carries a `closedAt` (GitHub closes it as it merges),
    /// which is ignored: the merge is the event the tab orders by.
    pub fn from_parts(
        state: PullState,
        merged_at: Option<DateTime<Utc>>,
        closed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, EndingError> {
        match state {
            PullState::Open => Err(EndingError::StillOpen),
            PullState::Merged => merged_at
                .map(|at| Self::Merged { at })
                .ok_or(EndingError::MergedWithoutTime),
            PullState::Closed => closed_at
                .map(|at| Self::Closed { at })
                .ok_or(EndingError::ClosedWithoutTime),
        }
    }

    /// When it ended: the merge time or the close time.
    pub fn at(self) -> DateTime<Utc> {
        match self {
            Self::Merged { at } | Self::Closed { at } => at,
        }
    }

    pub fn is_merged(self) -> bool {
        matches!(self, Self::Merged { .. })
    }

    /// The [`PullState`] this ending leaves the pull request in.
    pub fn state(self) -> PullState {
        match self {
            Self::Merged { .. } => PullState::Merged,
            Self::Closed { .. } => PullState::Closed,
        }
    }

    /// One word for a row or a chip.
    pub fn verb(self) -> &'static str {
        match self {
            Self::Merged { .. } => "merged",
            Self::Closed { .. } => "closed",
        }
    }
}

/// One closed pull request: what it was, and how it ended.
///
/// `pull` is the same [`PullRequest`] the open feed holds, so the detail pane
/// renders a closed one without a second model. Its merge fields
/// (`mergeable`, `merge_state`, `checks`, `base_divergence`) are never
/// fetched for a closed pull request and stay at their unknown values;
/// nothing on the Closed tab reads them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClosedPull {
    pub pull: PullRequest,
    pub ending: PullEnding,
}

impl ClosedPull {
    pub fn number(&self) -> PrNumber {
        self.pull.number
    }

    pub fn ended_at(&self) -> DateTime<Utc> {
        self.ending.at()
    }
}

/// Index into `RepoState::closed`, unfiltered, exactly like
/// [`crate::feed::PrIx`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClosedIx(pub usize);

impl FeedFilter {
    /// Whether the Closed tab shows `closed`, from repository `repo`.
    ///
    /// The author selection and the search apply as they do to open pull
    /// requests; the search also matches the repository's `owner/name`, since
    /// the tab mixes repositories in one list with no headers to scan.
    /// `hide_drafts` does not apply: a draft that was closed is no longer
    /// waiting on anyone, and hiding it would hide part of the history.
    pub fn accepts_closed(&self, repo: &RepoId, closed: &ClosedPull) -> bool {
        let pr = &closed.pull;
        if !self.accepts_author(|login| pr.is_authored_by(login), |login| pr.involves(login)) {
            return false;
        }
        pr.matches_query(&self.query) || repo_matches(repo, &self.query)
    }
}

fn repo_matches(repo: &RepoId, query: &str) -> bool {
    !query.is_empty()
        && repo
            .to_string()
            .to_lowercase()
            .contains(&query.to_lowercase())
}

/// Every closed pull request the filter lets through, across every
/// repository, newest ending first.
///
/// Ties — two pull requests merged in the same second, which a merge queue
/// does routinely — break on the repository, then on the higher number, so
/// the list never reshuffles between two refreshes that carry the same
/// times.
pub fn closed_order(repos: &[RepoState], filter: &FeedFilter) -> Vec<(RepoIx, ClosedIx)> {
    let mut visible: Vec<(RepoIx, ClosedIx)> = repos
        .iter()
        .enumerate()
        .flat_map(|(repo_ix, repo)| {
            repo.closed
                .iter()
                .enumerate()
                .filter(|(_, closed)| filter.accepts_closed(&repo.id, closed))
                .map(move |(closed_ix, _)| (RepoIx(repo_ix), ClosedIx(closed_ix)))
        })
        .collect();
    let lookup = |(repo, closed): &(RepoIx, ClosedIx)| {
        let state = &repos[repo.0];
        (&state.id, &state.closed[closed.0])
    };
    visible.sort_by(|a, b| {
        let (a_repo, a_closed) = lookup(a);
        let (b_repo, b_closed) = lookup(b);
        b_closed
            .ended_at()
            .cmp(&a_closed.ended_at())
            .then_with(|| a_repo.cmp(b_repo))
            .then_with(|| b_closed.number().cmp(&a_closed.number()))
    });
    visible
}

/// How many closed pull requests the filter lets through, across every
/// repository: the Closed tab's badge.
pub fn closed_count(repos: &[RepoState], filter: &FeedFilter) -> usize {
    repos
        .iter()
        .map(|repo| {
            repo.closed
                .iter()
                .filter(|closed| filter.accepts_closed(&repo.id, closed))
                .count()
        })
        .sum()
}

/// Whether a repository's closed list is due a fetch.
///
/// Closed pull requests are fetched only for the Closed tab: when it opens,
/// and on the poll while it is showing. Opening the tab again soon after a
/// fetch must not refetch, so a list younger than `max_age` is fresh. A
/// fetch already in flight is not due; a failed or never-attempted one is.
pub fn closed_needs_fetch(load: &LoadState, now: DateTime<Utc>, max_age: Duration) -> bool {
    match load {
        LoadState::Idle | LoadState::Failed { .. } => true,
        LoadState::Loading => false,
        LoadState::Loaded { at } => now.signed_duration_since(*at) >= max_age,
    }
}

/// The author roster for the Closed tab: the people who opened the closed
/// pull requests, ordered like [`crate::authors::roster`], with each
/// person's most recent ending standing in for their latest update.
pub fn closed_roster(
    repos: &[RepoState],
    viewer: Option<&User>,
    selected: &BTreeSet<LoginKey>,
) -> Vec<AuthorEntry> {
    let authored = repos
        .iter()
        .flat_map(|repo| &repo.closed)
        .map(|closed| (closed.pull.author.as_ref(), closed.ended_at()));
    roster_from(authored, viewer, selected)
}

/// The closed pull request a selection names, if this repository holds it.
pub fn find_closed(repo: &RepoState, number: PrNumber) -> Option<&ClosedPull> {
    repo.closed.iter().find(|closed| closed.number() == number)
}
