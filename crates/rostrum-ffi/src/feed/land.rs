//! Sending the feed's batches and applying each as it lands, through a weak
//! handle on the state actor so background work (probes) dies with the
//! core.

use std::collections::HashSet;

use futures::{StreamExt, stream};
use rostrum_core::RepoId;
use rostrum_github::{GitHubClient, RepoStacks};

use crate::{
    engine::actor::WeakActor,
    error::RostrumError,
    feed::batch::{self, CONCURRENT_BATCHES, Seq},
};

/// What a set of pull request batches did.
#[derive(Debug, Default)]
pub(crate) struct PullsLanded {
    /// Repositories that now hold their fresh pull requests.
    pub landed: Vec<Seq>,
    /// GitHub rejected the token.
    pub rejected: Option<String>,
}

/// Fetch and apply `fetches`' pull requests, publishing after each chunk.
pub(crate) async fn land_pulls(
    me: &WeakActor,
    client: &GitHubClient,
    fetches: Vec<Seq>,
    limit: u32,
) -> Result<PullsLanded, RostrumError> {
    let mut landing = batch::pulls(client, fetches, limit);
    let mut all = PullsLanded::default();
    while let Some((chunk, outcome)) = landing.next().await {
        let landed = me
            .call(move |state| {
                let landed = state.apply_pulls_batch(&chunk, outcome);
                state.publish();
                landed
            })
            .await?;
        all.landed.extend(landed.landed);
        all.rejected = all.rejected.or(landed.rejected);
    }
    Ok(all)
}

/// Fetch and apply `repos`' open issues, publishing after each chunk.
pub(crate) async fn land_issues(
    me: &WeakActor,
    client: &GitHubClient,
    repos: Vec<RepoId>,
    limit: u32,
) -> Result<(), RostrumError> {
    let mut landing = batch::issues(client, repos, limit);
    while let Some((chunk, outcome)) = landing.next().await {
        me.call(move |state| {
            state.apply_issues_batch(&chunk, outcome);
            state.publish();
        })
        .await?;
    }
    Ok(())
}

/// Ask how far the pull requests that just `landed` have drifted from their
/// bases, in as few documents as possible, and write the answers back.
pub(crate) async fn land_divergences(
    me: &WeakActor,
    client: &GitHubClient,
    landed: Vec<Seq>,
    limit: u32,
) -> Result<(), RostrumError> {
    let asks = me
        .call(move |state| {
            if state
                .paused_for(rostrum_core::budget::Feature::Divergence)
                .is_some()
            {
                return Vec::new();
            }
            state.divergence_asks(&landed)
        })
        .await?;
    let mut landing = batch::divergences(client, asks, limit);
    while let Some((chunk, outcome)) = landing.next().await {
        me.call(move |state| {
            state.apply_divergence_batch(&chunk, outcome);
            state.publish();
        })
        .await?;
    }
    Ok(())
}

/// Read the stacks of every repository that just `landed` with something to
/// stack, unless GitHub lately said stacks are off there. REST, one request
/// per repository.
pub(crate) async fn land_stacks(
    me: &WeakActor,
    client: &GitHubClient,
    landed: Vec<Seq>,
    skip: HashSet<RepoId>,
) -> Result<(), RostrumError> {
    let ids: Vec<RepoId> = landed.into_iter().map(|(repo, _)| repo).collect();
    // Not stackable: nothing open to stack, so whatever was known is moot.
    let asked: Vec<(RepoId, bool)> = me
        .call(move |state| {
            if state
                .paused_for(rostrum_core::budget::Feature::Stacks)
                .is_some()
            {
                return Vec::new();
            }
            ids.into_iter()
                .filter_map(|repo| {
                    let stackable = state
                        .feed
                        .repo(&repo)
                        .is_some_and(|held| !held.prs.is_empty());
                    if stackable && skip.contains(&repo) {
                        None
                    } else {
                        Some((repo, stackable))
                    }
                })
                .collect()
        })
        .await?;
    let mut landing = stream::iter(asked)
        .map(|(repo, stackable)| async move {
            let read = if stackable {
                client.stacks(&repo).await
            } else {
                Ok(RepoStacks::Available(Vec::new()))
            };
            (repo, read)
        })
        .buffer_unordered(CONCURRENT_BATCHES);
    while let Some((repo, read)) = landing.next().await {
        me.call(move |state| {
            state.apply_stacks_read(&repo, read);
            state.publish();
        })
        .await?;
    }
    Ok(())
}
