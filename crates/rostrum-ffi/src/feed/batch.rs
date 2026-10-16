//! The feed's requests: several repositories per GraphQL document, chunked
//! by `rostrum_github::batch::chunk_size` (the cheapest size per kind — pull
//! requests one repository at a time at 25 per repository, issues and checks
//! all together, divergence twenty to a document).
//!
//! Network only: each function sends its chunks a few at a time and yields
//! each chunk's answer as it lands, for the caller to apply through the state
//! actor.

use futures::{Stream, StreamExt, stream};
use rostrum_core::{Divergence, Issue, PrNumber, RepoId, ci::PrChecks};
use rostrum_github::{
    FetchedPulls, GitHubClient, GitHubError, RepoBatch,
    batch::{BatchKind, chunk_size, chunks, divergence::RepoPairs},
};

/// Documents in flight at once. GitHub's secondary rate limit punishes
/// bursts of concurrent requests; four keeps a thirty-repository refresh
/// quick without tripping it.
pub(crate) const CONCURRENT_BATCHES: usize = 4;

/// A repository and the sequence number its feed fetch was started with.
pub(crate) type Seq = (RepoId, u64);

/// `items` cut into the chunks `kind` is cheapest in at `first` per
/// repository.
fn chunked<T: Clone>(kind: BatchKind, first: u32, items: &[T]) -> Vec<Vec<T>> {
    let size = chunk_size(kind, first, items.len());
    chunks(items.len(), size)
        .map(|range| items[range].to_vec())
        .collect()
}

/// Open pull requests for `fetches`, one document per chunk.
pub(crate) fn pulls(
    client: &GitHubClient,
    fetches: Vec<Seq>,
    limit: u32,
) -> impl Stream<Item = (Vec<Seq>, Result<RepoBatch<FetchedPulls>, GitHubError>)> + '_ {
    stream::iter(chunked(BatchKind::OpenPulls, limit, &fetches))
        .map(move |chunk| async move {
            let ids: Vec<RepoId> = chunk.iter().map(|(id, _)| id.clone()).collect();
            let outcome = client.open_pull_requests_batch(&ids, limit).await;
            (chunk, outcome)
        })
        .buffer_unordered(CONCURRENT_BATCHES)
}

/// Open issues for `repos`, one document per chunk.
pub(crate) fn issues(
    client: &GitHubClient,
    repos: Vec<RepoId>,
    limit: u32,
) -> impl Stream<Item = (Vec<RepoId>, Result<RepoBatch<Vec<Issue>>, GitHubError>)> + '_ {
    stream::iter(chunked(BatchKind::Issues, limit, &repos))
        .map(move |chunk| async move {
            let outcome = client.open_issues_batch(&chunk, limit).await;
            (chunk, outcome)
        })
        .buffer_unordered(CONCURRENT_BATCHES)
}

/// Head-commit checks for `repos`, one document per chunk.
pub(crate) fn checks(
    client: &GitHubClient,
    repos: Vec<RepoId>,
    limit: u32,
) -> impl Stream<Item = (Vec<RepoId>, Result<RepoBatch<Vec<PrChecks>>, GitHubError>)> + '_ {
    stream::iter(chunked(BatchKind::Ci, limit, &repos))
        .map(move |chunk| async move {
            let outcome = client.ci_checks_batch(&chunk, limit).await;
            (chunk, outcome)
        })
        .buffer_unordered(CONCURRENT_BATCHES)
}

/// One repository's divergence question: its `(base, head)` pairs, the pull
/// requests they belong to (index-aligned), and the feed fetch whose heads
/// they were read from — an answer only applies while that fetch is still
/// the newest.
#[derive(Clone, Debug)]
pub(crate) struct DivergenceAsk {
    pub pairs: RepoPairs,
    pub numbers: Vec<PrNumber>,
    pub seq: u64,
}

/// One divergence chunk and its answer.
pub(crate) type DivergenceLanding = (
    Vec<DivergenceAsk>,
    Result<RepoBatch<Vec<Option<Divergence>>>, GitHubError>,
);

/// Distance from base for `asks`, one document per chunk.
pub(crate) fn divergences(
    client: &GitHubClient,
    asks: Vec<DivergenceAsk>,
    limit: u32,
) -> impl Stream<Item = DivergenceLanding> + '_ {
    stream::iter(chunked(BatchKind::Divergence, limit, &asks))
        .map(move |chunk| async move {
            let pairs: Vec<RepoPairs> = chunk.iter().map(|ask| ask.pairs.clone()).collect();
            let outcome = client.divergences_batch(&pairs).await;
            (chunk, outcome)
        })
        .buffer_unordered(CONCURRENT_BATCHES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repos(count: usize) -> Vec<RepoId> {
        (0..count)
            .map(|index| RepoId::new("o", format!("r{index}")))
            .collect()
    }

    #[test]
    fn pull_requests_go_one_repository_at_a_time_at_25() {
        let chunks = chunked(BatchKind::OpenPulls, 25, &repos(5));
        assert_eq!(chunks.len(), 5);
        assert!(chunks.iter().all(|chunk| chunk.len() == 1));
    }

    #[test]
    fn issues_and_checks_go_all_together() {
        assert_eq!(chunked(BatchKind::Issues, 25, &repos(5)).len(), 1);
        assert_eq!(chunked(BatchKind::Ci, 25, &repos(5)).len(), 1);
    }

    #[test]
    fn divergence_is_twenty_to_a_document() {
        let chunks = chunked(BatchKind::Divergence, 25, &repos(30));
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![20, 10]
        );
    }

    #[test]
    fn nothing_to_send_is_no_chunk() {
        assert!(chunked(BatchKind::Issues, 25, &repos(0)).is_empty());
    }
}
