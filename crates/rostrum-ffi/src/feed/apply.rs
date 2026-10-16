//! Applying the feed's batch answers to the state, one repository at a time:
//! pull requests (which decide whether a fetch is the newest), issues,
//! divergence and stacks, each cached.

use std::time::Instant;

use rostrum_core::{
    Divergence, Issue, PullRequest, RepoId, Stack, apply_divergences, divergence_query,
};
use rostrum_github::{
    FetchedPulls, GitHubError, RepoBatch, RepoStacks, batch::divergence::RepoPairs,
};

use crate::{
    budget::is_rate_limit,
    engine::{
        state::{CoreState, PullKey},
        writer::Write,
    },
    feed::{
        batch::{DivergenceAsk, Seq},
        state::{Applied, Fetched},
    },
};

/// What one chunk of pull requests did.
#[derive(Debug, Default)]
pub(crate) struct ChunkLanded {
    /// Repositories that now hold this chunk's answer, with its sequence.
    pub landed: Vec<Seq>,
    /// GitHub rejected the token.
    pub rejected: Option<String>,
}

impl CoreState {
    /// Apply one chunk's answer: the viewer it reported, then each
    /// repository's pull requests or failure. A failure of the whole
    /// document is each repository's.
    pub(crate) fn apply_pulls_batch(
        &mut self,
        chunk: &[Seq],
        outcome: Result<RepoBatch<FetchedPulls>, GitHubError>,
    ) -> ChunkLanded {
        let mut landed = ChunkLanded::default();
        match outcome {
            Ok(batch) => {
                tracing::debug!(
                    repos = chunk.len(),
                    cost = batch.rate_limit.as_ref().map(|limit| limit.cost),
                    remaining = batch.rate_limit.as_ref().map(|limit| limit.remaining),
                    "pull requests fetched"
                );
                if let Some(viewer) = batch.viewer {
                    self.session.verify(viewer);
                }
                for (repo, result) in batch.results {
                    let Some(seq) = seq_of(chunk, &repo) else {
                        continue;
                    };
                    let applied = match result {
                        Ok(fetched) => self.apply_pulls(&repo, seq, fetched),
                        Err(error) => self.fail_pulls(&repo, seq, &error),
                    };
                    if applied == Applied::Loaded {
                        landed.landed.push((repo, seq));
                    }
                }
            }
            Err(error) => {
                if matches!(error, GitHubError::Unauthorized) {
                    landed.rejected = Some(error.to_string());
                }
                for (repo, seq) in chunk {
                    self.fail_pulls(repo, *seq, &error);
                }
            }
        }
        landed
    }

    /// One repository's fresh pull requests, cached with its facts.
    fn apply_pulls(&mut self, repo: &RepoId, seq: u64, fetched: FetchedPulls) -> Applied {
        let applied = self.feed.apply_fetch(
            repo,
            seq,
            Ok(Fetched {
                prs: fetched.pull_requests,
                meta: fetched.meta,
            }),
        );
        if applied == Applied::Loaded {
            self.cache_pulls(repo);
            if let Some(meta) = self.feed.repo(repo).and_then(|state| state.meta.clone()) {
                self.writer.send(Write::RepoMeta {
                    repo: repo.clone(),
                    meta,
                });
            }
        }
        applied
    }

    /// One repository's failed fetch. A rate-limit refusal of a repository
    /// that has pull requests to show changes nothing: they stay, unmarked,
    /// and the budget readout says why nothing moves.
    pub(crate) fn fail_pulls(&mut self, repo: &RepoId, seq: u64, error: &GitHubError) -> Applied {
        tracing::warn!(%repo, %error, "refresh failed");
        self.note_github_error(error);
        if is_rate_limit(error)
            && self
                .feed
                .repo(repo)
                .is_some_and(|state| !state.prs.is_empty())
        {
            return Applied::Failed;
        }
        self.feed.apply_fetch(repo, seq, Err(error))
    }

    /// Write a repository's pull requests as held to the cache.
    fn cache_pulls(&mut self, repo: &RepoId) {
        if let Some(state) = self.feed.repo(repo) {
            self.writer.send(Write::PullRequests {
                repo: repo.clone(),
                prs: state.prs.clone(),
            });
        }
    }

    /// Apply one chunk of issue answers, each cached. A failure keeps the
    /// issues already held and reports itself on the Issues tab only.
    pub(crate) fn apply_issues_batch(
        &mut self,
        chunk: &[RepoId],
        outcome: Result<RepoBatch<Vec<Issue>>, GitHubError>,
    ) {
        match outcome {
            Ok(batch) => {
                for (repo, result) in batch.results {
                    self.apply_issues(&repo, result);
                }
            }
            Err(error) => {
                for repo in chunk {
                    self.fail_issues(repo, &error);
                }
            }
        }
    }

    fn apply_issues(&mut self, repo: &RepoId, outcome: Result<Vec<Issue>, GitHubError>) {
        match outcome {
            Ok(issues) => {
                self.feed.apply_issues(repo, Ok(issues.clone()));
                self.writer.send(Write::Issues {
                    repo: repo.clone(),
                    issues,
                });
            }
            Err(error) => self.fail_issues(repo, &error),
        }
    }

    fn fail_issues(&mut self, repo: &RepoId, error: &GitHubError) {
        tracing::warn!(%repo, %error, "issue refresh failed");
        self.note_github_error(error);
        if is_rate_limit(error)
            && self
                .feed
                .repo(repo)
                .is_some_and(|state| !state.issues.is_empty())
        {
            return;
        }
        self.feed.apply_issues(repo, Err(error));
    }

    /// The divergence questions for the repositories whose pull requests
    /// just landed, from the heads they hold now. Repositories without a
    /// pull request to compare are left out.
    pub(crate) fn divergence_asks(&self, landed: &[Seq]) -> Vec<DivergenceAsk> {
        landed
            .iter()
            .filter(|(repo, seq)| self.feed.applied_seq(repo) == Some(*seq))
            .filter_map(|(repo, seq)| {
                let state = self.feed.repo(repo)?;
                let (numbers, pairs) = divergence_query(&state.prs);
                (!pairs.is_empty()).then(|| DivergenceAsk {
                    pairs: RepoPairs {
                        repo: repo.clone(),
                        pairs,
                    },
                    numbers,
                    seq: *seq,
                })
            })
            .collect()
    }

    /// Write a chunk of divergence answers onto the pull requests they were
    /// asked about, matched by number, and only while the feed fetch they
    /// were read from is still the newest — a newer list may have moved a
    /// head. A failure keeps the counts already shown.
    pub(crate) fn apply_divergence_batch(
        &mut self,
        chunk: &[DivergenceAsk],
        outcome: Result<RepoBatch<Vec<Option<Divergence>>>, GitHubError>,
    ) {
        let batch = match outcome {
            Ok(batch) => batch,
            Err(error) => {
                tracing::debug!(repos = chunk.len(), %error, "divergence batch failed; keeping previous counts");
                return;
            }
        };
        for (repo, result) in batch.results {
            let Some(ask) = chunk.iter().find(|ask| ask.pairs.repo == repo) else {
                continue;
            };
            if self.feed.applied_seq(&repo) != Some(ask.seq) {
                continue;
            }
            match result {
                Ok(divergences) => {
                    let Some(state) = self.feed.repo_mut(&repo) else {
                        continue;
                    };
                    apply_divergences(&mut state.prs, &ask.numbers, divergences);
                    let prs: Vec<PullRequest> = state.prs.clone();
                    for pr in prs {
                        self.feed.known.insert(
                            PullKey {
                                repo: repo.clone(),
                                number: pr.number,
                            },
                            pr,
                        );
                    }
                    self.cache_pulls(&repo);
                }
                Err(error) => {
                    tracing::debug!(%repo, %error, "divergence failed; keeping previous counts");
                }
            }
        }
    }

    /// Apply a stacks read.
    pub(crate) fn apply_stacks_read(
        &mut self,
        repo: &RepoId,
        read: Result<RepoStacks, GitHubError>,
    ) {
        let stacks: Option<Vec<Stack>> = match read {
            Ok(RepoStacks::Available(stacks)) => {
                self.stacks_unavailable.remove(repo);
                Some(stacks)
            }
            Ok(RepoStacks::Unavailable) => {
                tracing::debug!(%repo, "stacked pull requests are not enabled");
                self.stacks_unavailable.insert(repo.clone(), Instant::now());
                Some(Vec::new())
            }
            // A failed read keeps what was known: a stack does not stop being
            // one because one poll could not ask.
            Err(error) => {
                tracing::debug!(%repo, %error, "stacks read failed; keeping the last answer");
                None
            }
        };
        if let Some(stacks) = stacks
            && self.feed.apply_stacks(repo, stacks.clone())
        {
            self.writer.send(Write::Stacks {
                repo: repo.clone(),
                stacks,
            });
        }
    }
}

fn seq_of(chunk: &[Seq], repo: &RepoId) -> Option<u64> {
    chunk.iter().find(|(id, _)| id == repo).map(|(_, seq)| *seq)
}
