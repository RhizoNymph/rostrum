//! The store's pull request half: open pull requests fetched in batches,
//! and what follows once a poll's batches have all landed — one divergence
//! batch, the stacks, the CI checks that could have changed, and the merge
//! probes.
//!
//! Follow-ups wait for the last feed batch so that every repository's
//! divergence rides one document (one point, however many repositories) and
//! the CI planner sees every new head at once.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{
    MergeProbeBudget, PrNumber, RepoId, apply_divergences, budget::Feature,
    carry_forward_divergence, divergence_query, needs_merge_probe, poll::Trigger,
};
use rostrum_github::{
    FetchedPulls, GitHubError, RepoBatch,
    batch::{BatchKind, chunk_size, chunks, divergence::RepoPairs},
};

use super::{Store, inflight::BatchId, is_rate_limit};

/// Why a feed batch was sent; decides what follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeedOrigin {
    /// A poll, a refresh, or one repository after a change: the follow-ups
    /// run when it lands.
    Poll,
    /// A re-query for merge states GitHub was still computing. Heads have
    /// not moved in the seconds since, so divergence, stacks and checks are
    /// not asked again.
    Probe,
}

/// What the feed's follow-ups need between batches.
#[derive(Default)]
pub(super) struct FeedCycle {
    /// Repositories whose pull requests landed since the follow-ups last ran.
    landed: BTreeSet<RepoId>,
    /// The cycle was asked for (the Refresh action): checks are fetched
    /// whatever the skip rules say.
    explicit: bool,
    /// Merge probes left this poll cycle, shared by every repository: one
    /// probe re-queries every repository still computing, in one batch.
    probe_budget: MergeProbeBudget,
    /// A probe's timer is running.
    probe_pending: bool,
    probe_task: Option<Task<()>>,
    /// Divergence batches in flight, and the newest batch asked for each
    /// repository: an older batch's answer for a repository is dropped
    /// rather than written over a newer one.
    divergence_tasks: BTreeMap<u64, Task<()>>,
    divergence_latest: HashMap<RepoId, u64>,
    divergence_next: u64,
}

impl FeedCycle {
    /// A poll starts a new cycle: every repository may be probed again.
    pub(super) fn begin(&mut self, trigger: Trigger) {
        self.probe_budget = MergeProbeBudget::default();
        if trigger == Trigger::Explicit {
            self.explicit = true;
        }
    }

    pub(super) fn forget(&mut self, id: &RepoId) {
        self.landed.remove(id);
        self.divergence_latest.remove(id);
    }
}

type FeedOutcome = Result<Result<RepoBatch<FetchedPulls>, GitHubError>, gpui_tokio::JoinError>;
type DivergenceOutcome = Result<
    Result<RepoBatch<Vec<Option<rostrum_core::Divergence>>>, GitHubError>,
    gpui_tokio::JoinError,
>;

impl Store {
    /// Refresh one repository's pull requests (after adding it, or after a
    /// change made from rostrum), followed by its divergence, stacks and
    /// checks.
    pub fn refresh_repo(&mut self, id: RepoId, cx: &mut Context<Self>) {
        self.refresh_feed(vec![id], FeedOrigin::Poll, cx);
    }

    /// Fetch the open pull requests of `ids`, leaving out any already in
    /// flight, in as many batches as [`chunk_size`] says is cheapest.
    pub(super) fn refresh_feed(
        &mut self,
        ids: Vec<RepoId>,
        origin: FeedOrigin,
        cx: &mut Context<Self>,
    ) {
        let feature = match origin {
            FeedOrigin::Poll => Feature::Feed,
            FeedOrigin::Probe => Feature::Probes,
        };
        let Some(client) = self.client_for(feature) else {
            return;
        };
        if self.paused(feature) {
            return;
        }
        let known: Vec<RepoId> = ids
            .into_iter()
            .filter(|id| self.state.repo(id).is_some())
            .collect();
        let ids = self.feed.claimable(known);
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            // Only a first load shows a spinner; a background refresh must
            // not blank a card that already has data.
            if let Some(repo) = self.state.repo_mut(id)
                && repo.prs.is_empty()
            {
                repo.load = rostrum_core::LoadState::Loading;
            }
        }
        cx.notify();

        let limit = self.config.prs_per_repo;
        let size = chunk_size(BatchKind::OpenPulls, limit, ids.len());
        for range in chunks(ids.len(), size) {
            let chunk = ids[range].to_vec();
            let batch = self.feed.next_id();
            let client = client.clone();
            let fetch = chunk.clone();
            let apply = chunk.clone();
            let task = cx.spawn(async move |this, cx| {
                let outcome = Tokio::spawn(&*cx, async move {
                    client.open_pull_requests_batch(&fetch, limit).await
                })
                .await;
                // No await points after this: `apply_feed_batch` releases
                // this task's own handle.
                this.update(cx, |this, cx| {
                    this.apply_feed_batch(batch, apply, origin, outcome, cx)
                })
                .ok();
            });
            self.feed.start(batch, chunk, task);
        }
    }

    fn apply_feed_batch(
        &mut self,
        batch: BatchId,
        chunk: Vec<RepoId>,
        origin: FeedOrigin,
        outcome: FeedOutcome,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        match outcome {
            Ok(Ok(answer)) => {
                // Every feed batch carries it; whichever arrives first wins
                // and the rest agree, since it is a property of the token.
                if let Some(viewer) = answer.viewer {
                    if self.viewer.as_ref() != Some(&viewer) {
                        tracing::debug!(login = %viewer.login, "resolved the viewer");
                    }
                    self.viewer = Some(viewer);
                }
                tracing::debug!(
                    repos = chunk.len(),
                    ?origin,
                    cost = answer.rate_limit.as_ref().map(|limit| limit.cost),
                    remaining = answer.rate_limit.as_ref().map(|limit| limit.remaining),
                    "pull requests refreshed"
                );
                for (id, result) in answer.results {
                    match result {
                        Ok(fetched) => {
                            self.apply_pulls(&id, fetched, now, cx);
                            if origin == FeedOrigin::Poll {
                                self.cycle.landed.insert(id);
                            }
                        }
                        Err(error) => self.feed_failed(&id, &error, now),
                    }
                }
            }
            Ok(Err(error)) => {
                for id in &chunk {
                    self.feed_failed(id, &error, now);
                }
            }
            Err(error) => {
                tracing::warn!(repos = chunk.len(), %error, "refresh task did not complete");
                for id in &chunk {
                    if let Some(repo) = self.state.repo_mut(id) {
                        repo.load = rostrum_core::LoadState::Failed {
                            message: format!("refresh did not complete: {error}"),
                            at: now,
                        };
                    }
                }
            }
        }

        self.feed.finish(batch);
        if self.feed.is_empty() {
            self.feed_settled(cx);
        }
        cx.notify();
    }

    /// Write one repository's fresh pull requests and cache them.
    fn apply_pulls(
        &mut self,
        id: &RepoId,
        mut fetched: FetchedPulls,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.state.repo_mut(id) else {
            return;
        };
        // The feed query never carries divergence; keep what the last batch
        // found until the next one answers.
        carry_forward_divergence(&repo.prs, &mut fetched.pull_requests);
        repo.prs = fetched.pull_requests;
        // An answer without metadata keeps the last known metadata rather
        // than dropping the repository to the bottom of a sort for one poll.
        if fetched.meta.is_some() {
            repo.meta = fetched.meta;
        }
        repo.load = rostrum_core::LoadState::Loaded { at: now };

        if let Some(db) = self.db.clone() {
            let repo = id.clone();
            let (prs, meta) = self
                .state
                .repo(&repo)
                .map(|repo| (repo.prs.clone(), repo.meta.clone()))
                .unwrap_or_default();
            // Fire and forget: a cache write failing must not disturb the
            // refresh that produced it. sqlx needs the Tokio reactor, so this
            // goes through the bridge rather than GPUI's executor.
            Tokio::spawn(&*cx, async move {
                if let Err(error) = db.save_pull_requests(&repo, &prs).await {
                    tracing::warn!(%repo, %error, "could not cache pull requests");
                }
                if let Some(meta) = meta
                    && let Err(error) = db.save_repo_meta(&repo, &meta).await
                {
                    tracing::warn!(%repo, %error, "could not cache repository metadata");
                }
            })
            .detach();
        }
    }

    /// One repository's refresh failed. A rate-limit refusal is not the
    /// repository's fault: whatever it showed stays, unmarked, and the
    /// status bar says polling is paused.
    fn feed_failed(&mut self, id: &RepoId, error: &GitHubError, now: DateTime<Utc>) {
        tracing::warn!(repo = %id, %error, "refresh failed");
        let Some(repo) = self.state.repo_mut(id) else {
            return;
        };
        if is_rate_limit(error) && !repo.prs.is_empty() {
            return;
        }
        repo.load = rostrum_core::LoadState::Failed {
            message: error.to_string(),
            at: now,
        };
    }

    /// The last feed batch has landed: run the follow-ups for every
    /// repository that landed since they last ran.
    fn feed_settled(&mut self, cx: &mut Context<Self>) {
        let landed = std::mem::take(&mut self.cycle.landed);
        let trigger = if std::mem::take(&mut self.cycle.explicit) {
            Trigger::Explicit
        } else {
            Trigger::Timer
        };
        if !landed.is_empty() {
            self.fetch_divergences(&landed, cx);
            for id in &landed {
                self.fetch_stacks(id, cx);
            }
            self.refresh_ci_planned(Some(&landed), trigger, cx);
        }
        self.schedule_merge_probe(cx);
    }

    /// Repositories with a pull request whose merge state GitHub is still
    /// computing.
    fn computing_repos(&self) -> Vec<RepoId> {
        self.state
            .repos
            .iter()
            .filter(|repo| needs_merge_probe(&repo.prs))
            .map(|repo| repo.id.clone())
            .collect()
    }

    /// Re-query, shortly, every repository that saw merge states GitHub had
    /// not finished computing.
    ///
    /// GitHub computes `mergeable` and `mergeStateStatus` lazily: the query
    /// that asks for them returns `UNKNOWN` *and* starts the computation, so
    /// a single poll can never see the answer. Probes back off 2 s, 4 s,
    /// 8 s and stop for the cycle after three — the steady state for a
    /// repository whose merge state the token may not read. One probe covers
    /// every repository still computing, in one batch.
    fn schedule_merge_probe(&mut self, cx: &mut Context<Self>) {
        if self.cycle.probe_pending {
            return;
        }
        let computing = self.computing_repos();
        if computing.is_empty() {
            return;
        }
        // Back off, because the wait is GitHub finishing a background job
        // and the second attempt is evidence the first was too early.
        let Some(delay) = self.cycle.probe_budget.next_delay() else {
            tracing::debug!(
                repos = computing.len(),
                "merge state still unknown; waiting for the next poll"
            );
            return;
        };
        tracing::debug!(
            repos = computing.len(),
            attempt = self.cycle.probe_budget.spent(),
            delay_ms = delay.as_millis(),
            "merge state computing; scheduling a probe"
        );
        self.cycle.probe_pending = true;
        self.cycle.probe_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                this.cycle.probe_pending = false;
                let computing = this.computing_repos();
                this.refresh_feed(computing, FeedOrigin::Probe, cx);
            })
            .ok();
        }));
    }

    /// Ask GitHub how far each landed repository's pull requests have
    /// drifted from their bases, in as few documents as possible, and write
    /// the answers back.
    ///
    /// Separate from the feed query because `Ref.compare` is not reachable
    /// from a `PullRequest` node; it hangs off the base ref, so it has to be
    /// a second document built from the refs the feed just returned.
    fn fetch_divergences(&mut self, ids: &BTreeSet<RepoId>, cx: &mut Context<Self>) {
        let Some(client) = self.client_for(Feature::Divergence) else {
            return;
        };
        if self.paused(Feature::Divergence) {
            return;
        }
        let asked: Vec<(RepoPairs, Vec<PrNumber>)> = ids
            .iter()
            .filter_map(|id| self.state.repo(id))
            .map(|repo| {
                let (numbers, pairs) = divergence_query(&repo.prs);
                (
                    RepoPairs {
                        repo: repo.id.clone(),
                        pairs,
                    },
                    numbers,
                )
            })
            .filter(|(entry, _)| !entry.pairs.is_empty())
            .collect();
        if asked.is_empty() {
            return;
        }

        let size = chunk_size(BatchKind::Divergence, self.config.prs_per_repo, asked.len());
        for range in chunks(asked.len(), size) {
            let chunk = &asked[range];
            self.cycle.divergence_next += 1;
            let seq = self.cycle.divergence_next;
            for (entry, _) in chunk {
                self.cycle.divergence_latest.insert(entry.repo.clone(), seq);
            }
            let requests: Vec<RepoPairs> = chunk.iter().map(|(entry, _)| entry.clone()).collect();
            let numbers: HashMap<RepoId, Vec<PrNumber>> = chunk
                .iter()
                .map(|(entry, numbers)| (entry.repo.clone(), numbers.clone()))
                .collect();
            let client = client.clone();
            let task = cx.spawn(async move |this, cx| {
                let outcome =
                    Tokio::spawn(
                        &*cx,
                        async move { client.divergences_batch(&requests).await },
                    )
                    .await;
                // No await points after this: `apply_divergence_batch`
                // releases this task's own handle.
                this.update(cx, |this, cx| {
                    this.apply_divergence_batch(seq, numbers, outcome, cx)
                })
                .ok();
            });
            self.cycle.divergence_tasks.insert(seq, task);
        }
    }

    /// Write a batch of divergence answers onto the pull requests they were
    /// asked about.
    ///
    /// Matched by number rather than by position: a refresh may have landed
    /// while the batch was in flight, reordering or replacing the list, and a
    /// count written to the wrong row is worse than none.
    ///
    /// A failed batch is logged at debug and changes nothing. The counts on
    /// screen were right a minute ago and are still the best available
    /// answer; blanking them would turn a transient error into a visible
    /// flicker on every poll that hits one.
    fn apply_divergence_batch(
        &mut self,
        seq: u64,
        numbers: HashMap<RepoId, Vec<PrNumber>>,
        outcome: DivergenceOutcome,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(Ok(answer)) => {
                for (id, result) in answer.results {
                    if self.cycle.divergence_latest.get(&id) != Some(&seq) {
                        continue;
                    }
                    match result {
                        Ok(divergences) => {
                            if let (Some(repo), Some(numbers)) =
                                (self.state.repo_mut(&id), numbers.get(&id))
                            {
                                apply_divergences(&mut repo.prs, numbers, divergences);
                                tracing::debug!(repo = %id, count = numbers.len(), "divergence updated");
                            }
                        }
                        Err(error) => {
                            tracing::debug!(repo = %id, %error, "divergence failed; keeping previous counts");
                        }
                    }
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(repos = numbers.len(), %error, "divergence batch failed; keeping previous counts");
            }
            Err(error) => {
                tracing::debug!(repos = numbers.len(), %error, "divergence batch did not complete");
            }
        }
        self.cycle
            .divergence_latest
            .retain(|_, latest| *latest != seq);
        self.cycle.divergence_tasks.remove(&seq);
        cx.notify();
    }
}
