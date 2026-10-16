//! What a batch costs, and how many repositories to put in one.
//!
//! GitHub's documented calculation: add up the requests needed to fulfil
//! each connection (a connection under `n` parents needs `n` requests at up
//! to 100 items each), divide by 100, round to the nearest whole number, and
//! charge at least one point — per document. Node counts are capped at
//! 500,000 per document.
//!
//! For the documents here, at `first` items per repository:
//!
//! | kind | requests per repository | nodes per repository |
//! |---|---|---|
//! | open pull requests | 1 + 5·first (assignees, review requests, labels, last commit, last force push) | 33·first |
//! | open issues | 1 + 2·first (assignees, labels) | 21·first |
//! | CI checks | 1 + 2·first (last commit, its contexts) | 102·first |
//! | divergence | 0 (`Ref.compare` is not a connection) | 0 |
//!
//! Checked against GitHub's own `rateLimit(dryRun: true) { cost nodeCount }`
//! at first = 25 for 1, 5, 10 and 20 repositories: pull requests cost 1, 6,
//! 13, 25 (825 nodes each), issues 1, 3, 5, 10 (525), CI 1, 3, 5, 10 (2,550),
//! and a 40-repository divergence document 1.

use std::ops::Range;

/// The batched kinds of document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchKind {
    OpenPulls,
    Issues,
    Ci,
    Divergence,
}

/// Most repositories in one document, whatever the arithmetic says: keeps
/// one slow or failing document's blast radius and response size bounded.
pub const MAX_CHUNK: usize = 20;

/// Most nodes in one document: a tenth of GitHub's 500,000 cap, so responses
/// stay a few megabytes at most.
pub const NODE_BUDGET: u64 = 50_000;

/// GitHub's requests for one repository's selection at `first` items.
pub fn repo_requests(kind: BatchKind, first: u32) -> u64 {
    let first = u64::from(first);
    match kind {
        BatchKind::OpenPulls => 1 + 5 * first,
        BatchKind::Issues | BatchKind::Ci => 1 + 2 * first,
        BatchKind::Divergence => 0,
    }
}

/// Nodes one repository's selection can return at `first` items.
pub fn repo_nodes(kind: BatchKind, first: u32) -> u64 {
    let first = u64::from(first);
    match kind {
        BatchKind::OpenPulls => 33 * first,
        BatchKind::Issues => 21 * first,
        BatchKind::Ci => 102 * first,
        BatchKind::Divergence => 0,
    }
}

/// Points one document over `repos` repositories costs.
pub fn chunk_cost(kind: BatchKind, first: u32, repos: usize) -> u64 {
    let requests = repo_requests(kind, first) * repos as u64;
    // Round half up, then GitHub's floor of one point.
    (requests.saturating_add(50) / 100).max(1)
}

/// Points `count` repositories cost sent `size` at a time.
pub fn total_cost(kind: BatchKind, first: u32, count: usize, size: usize) -> u64 {
    chunks(count, size)
        .map(|chunk| chunk_cost(kind, first, chunk.len()))
        .sum()
}

/// How many repositories to send per document: the size, within
/// [`MAX_CHUNK`] and [`NODE_BUDGET`], that costs least for `count`
/// repositories, the largest such size on a tie (fewer requests for the
/// same points). At least one.
pub fn chunk_size(kind: BatchKind, first: u32, count: usize) -> usize {
    let nodes = repo_nodes(kind, first).max(1);
    let node_cap = usize::try_from(NODE_BUDGET / nodes).unwrap_or(usize::MAX);
    let largest = MAX_CHUNK.min(node_cap).min(count).max(1);
    (1..=largest)
        .min_by_key(|&size| {
            (
                total_cost(kind, first, count, size),
                std::cmp::Reverse(size),
            )
        })
        .unwrap_or(1)
}

/// What a poll's work is, for estimating its cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollShape {
    pub repos: usize,
    pub prs_per_repo: u32,
    pub issues_per_repo: u32,
    /// Seconds between polls, between issue refreshes, and the CI safety
    /// interval.
    pub feed_secs: u64,
    pub issue_secs: u64,
    pub ci_secs: u64,
}

/// Roughly what one poll costs in points, averaged over the slower
/// cadences: every poll's pull requests and divergence, plus each poll's
/// share of the issue refresh and of the CI safety net. A floor, since CI
/// re-fetches for moved heads and running checks come on top; it is what
/// the pacing divides the remaining budget by.
pub fn poll_cost_estimate(shape: PollShape) -> u32 {
    let cost = |kind, first| {
        let size = chunk_size(kind, first, shape.repos);
        total_cost(kind, first, shape.repos, size)
    };
    if shape.repos == 0 {
        return 0;
    }
    let share = |points: u64, every: u64| {
        (points * shape.feed_secs.max(1)).div_ceil(every.max(shape.feed_secs).max(1))
    };
    let points = cost(BatchKind::OpenPulls, shape.prs_per_repo)
        + cost(BatchKind::Divergence, shape.prs_per_repo)
        + share(
            cost(BatchKind::Issues, shape.issues_per_repo),
            shape.issue_secs,
        )
        + share(cost(BatchKind::Ci, shape.prs_per_repo), shape.ci_secs);
    u32::try_from(points).unwrap_or(u32::MAX)
}

/// `0..count` cut into consecutive ranges of `size` (the last may be
/// shorter).
pub fn chunks(count: usize, size: usize) -> impl Iterator<Item = Range<usize>> {
    let size = size.max(1);
    (0..count)
        .step_by(size)
        .map(move |start| start..(start + size).min(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GitHub's dry-run answers at first = 25, captured live.
    #[test]
    fn the_model_matches_githubs_dry_run_costs() {
        for (kind, expected) in [
            (BatchKind::OpenPulls, [1, 6, 13, 25]),
            (BatchKind::Issues, [1, 3, 5, 10]),
            (BatchKind::Ci, [1, 3, 5, 10]),
            (BatchKind::Divergence, [1, 1, 1, 1]),
        ] {
            let costs = [1, 5, 10, 20].map(|repos| chunk_cost(kind, 25, repos));
            assert_eq!(costs, expected, "{kind:?}");
        }
        assert_eq!(repo_nodes(BatchKind::OpenPulls, 25), 825);
        assert_eq!(repo_nodes(BatchKind::Issues, 25), 525);
        assert_eq!(repo_nodes(BatchKind::Ci, 25), 2550);
    }

    #[test]
    fn pull_requests_go_one_repository_at_a_time_at_the_default_page() {
        // 1.26 points each: alone it rounds down to 1, together it does not.
        assert_eq!(chunk_size(BatchKind::OpenPulls, 25, 5), 1);
        assert_eq!(total_cost(BatchKind::OpenPulls, 25, 5, 1), 5);
        assert_eq!(chunk_size(BatchKind::OpenPulls, 25, 30), 1);
    }

    #[test]
    fn issues_and_checks_go_together() {
        assert_eq!(chunk_size(BatchKind::Issues, 25, 5), 5);
        assert_eq!(total_cost(BatchKind::Issues, 25, 5, 5), 3);
        assert_eq!(chunk_size(BatchKind::Ci, 25, 5), 5);
        assert_eq!(total_cost(BatchKind::Ci, 25, 5, 5), 3);
    }

    #[test]
    fn a_larger_page_of_pull_requests_batches_in_pairs() {
        // 2.51 points each: alone 3, two together 5.
        assert_eq!(chunk_cost(BatchKind::OpenPulls, 50, 1), 3);
        assert_eq!(chunk_cost(BatchKind::OpenPulls, 50, 2), 5);
        let size = chunk_size(BatchKind::OpenPulls, 50, 4);
        assert_eq!(total_cost(BatchKind::OpenPulls, 50, 4, size), 10);
        assert!(size >= 2);
    }

    #[test]
    fn the_node_budget_bounds_a_chunk() {
        // 100 checks pages: 10,200 nodes a repository, four fit.
        assert!(chunk_size(BatchKind::Ci, 100, 40) <= 4);
        assert_eq!(chunk_size(BatchKind::Divergence, 25, 100), MAX_CHUNK);
    }

    #[test]
    fn a_chunk_is_at_least_one_and_at_most_the_count() {
        assert_eq!(chunk_size(BatchKind::Issues, 25, 0), 1);
        assert_eq!(chunk_size(BatchKind::Issues, 25, 1), 1);
        assert_eq!(chunk_size(BatchKind::Divergence, 25, 3), 3);
    }

    #[test]
    fn a_poll_costs_its_feed_divergence_and_shares_of_the_rest() {
        let shape = PollShape {
            repos: 5,
            prs_per_repo: 25,
            issues_per_repo: 25,
            feed_secs: 60,
            issue_secs: 300,
            ci_secs: 600,
        };
        // 5 feed + 1 divergence + ceil(3 / 5) + ceil(3 / 10).
        assert_eq!(poll_cost_estimate(shape), 8);
        assert_eq!(poll_cost_estimate(PollShape { repos: 0, ..shape }), 0);
        // Issues as often as the feed: their whole cost every poll.
        let eager = PollShape {
            issue_secs: 60,
            ..shape
        };
        assert_eq!(poll_cost_estimate(eager), 10);
    }

    #[test]
    fn chunks_cover_every_index_once() {
        assert_eq!(chunks(5, 2).collect::<Vec<_>>(), vec![0..2, 2..4, 4..5]);
        assert_eq!(chunks(4, 4).collect::<Vec<_>>(), vec![0..4]);
        assert_eq!(chunks(0, 3).count(), 0);
        assert_eq!(chunks(3, 0).collect::<Vec<_>>(), vec![0..1, 1..2, 2..3]);
    }
}
