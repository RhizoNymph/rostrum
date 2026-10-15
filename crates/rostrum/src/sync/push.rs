//! Push all: every checked-out pull request branch of every repository with
//! a clone, pushed to its head repository.
//!
//! Three phases, one at a time across the app (it shares the sync toolbar's
//! "one thing at a time" with the local syncs):
//!
//! 1. **Planning.** Every branch is fetched and planned
//!    ([`rostrum_local::plan_push_all`]).
//! 2. **Confirming**, only when some branch has diverged from its upstream.
//!    The feed shows one confirmation listing every branch that would be
//!    rewritten, with the remote oid each would replace; the user answers
//!    once for the batch ([`RewriteDecision`]) or cancels. Fast-forwards
//!    never need confirming.
//! 3. **Running.** Each authorized step runs [`rostrum_local::run_push`] in
//!    turn, which plans again from the refs on disk and pushes only if the
//!    branch is still what was confirmed. Verdicts land on the feed rows as
//!    they arrive; repositories that received a push are refreshed.

use std::collections::{BTreeMap, BTreeSet};

use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{PrNumber, RepoId};
use rostrum_git::BranchName;
use rostrum_local::{
    PushAllPlan, PushResult, PushSkip, PushStep, PushTarget, RewriteDecision, plan_push_all,
    run_push,
};

use super::Store;

/// One pull request in a Push all.
pub type PushKey = (RepoId, PrNumber);

/// Where a Push all is.
pub enum PushAll {
    /// Fetching and planning `total` branches.
    Planning { total: usize },
    /// Planned, with at least one rewrite; waiting for the user's answer.
    Confirming {
        plan: PushAllPlan<PushKey>,
        /// Pull requests whose head name git would refuse, settled before
        /// planning.
        prefailed: Vec<(PushKey, PushResult)>,
    },
    /// Running, or finished once `done == total`.
    Running(PushProgress),
}

/// A running or finished Push all's verdicts.
#[derive(Clone, Debug)]
pub struct PushProgress {
    pub done: usize,
    pub total: usize,
    pub results: BTreeMap<PushKey, PushResult>,
}

impl PushProgress {
    pub fn is_finished(&self) -> bool {
        self.done >= self.total
    }

    pub fn summary(&self) -> PushSummary {
        summarise(self.results.values())
    }
}

/// How many pull requests landed in each reportable bucket. Up to date and
/// not checked out are not counted, as in a local sync's summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushSummary {
    pub pushed: usize,
    pub rewritten: usize,
    pub skipped: usize,
    pub not_confirmed: usize,
    pub refused: usize,
    pub failed: usize,
}

impl PushSummary {
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            (self.pushed, "pushed"),
            (self.rewritten, "rewritten"),
            (self.skipped, "skipped"),
            (self.not_confirmed, "not confirmed"),
            (self.refused, "refused"),
            (self.failed, "failed"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, noun)| format!("{count} {noun}"))
        .collect();
        if parts.is_empty() {
            "nothing to push".to_string()
        } else {
            parts.join(", ")
        }
    }
}

fn summarise<'a>(results: impl Iterator<Item = &'a PushResult>) -> PushSummary {
    let mut summary = PushSummary::default();
    for result in results {
        match result {
            PushResult::Pushed { rewrote: false, .. } => summary.pushed += 1,
            PushResult::Pushed { rewrote: true, .. } => summary.rewritten += 1,
            PushResult::Skipped(PushSkip::UpToDate | PushSkip::NotCheckedOut) => {}
            PushResult::Skipped(_) => summary.skipped += 1,
            PushResult::NotConfirmed { .. } => summary.not_confirmed += 1,
            PushResult::ConfirmationStale { .. } | PushResult::Rejected(_) => summary.refused += 1,
            PushResult::Failed(_) => summary.failed += 1,
        }
    }
    summary
}

/// The store's Push all, and the task driving it.
#[derive(Default)]
pub struct PushAllSync {
    state: Option<PushAll>,
    /// Dropping it cancels between branches — never mid-push, because each
    /// push runs to completion on the Tokio side.
    task: Option<Task<()>>,
}

impl PushAllSync {
    /// Planning, waiting on the user, or running: the toolbar is busy.
    pub fn is_active(&self) -> bool {
        match &self.state {
            None => false,
            Some(PushAll::Planning { .. } | PushAll::Confirming { .. }) => true,
            Some(PushAll::Running(progress)) => !progress.is_finished(),
        }
    }

    pub fn state(&self) -> Option<&PushAll> {
        self.state.as_ref()
    }

    /// The latest verdict for one pull request, once a run has started.
    pub fn result(&self, key: &PushKey) -> Option<&PushResult> {
        match &self.state {
            Some(PushAll::Running(progress)) => progress.results.get(key),
            _ => None,
        }
    }

    /// Forget the last run's verdicts, so a local sync's replace them.
    pub fn clear(&mut self) {
        self.state = None;
        self.task = None;
    }
}

impl Store {
    /// The Push all, if one has run or is running.
    pub fn push_all_state(&self) -> Option<&PushAll> {
        self.push_all.state()
    }

    /// Start a Push all: plan every checked-out pull request branch of every
    /// repository with a clone, then confirm or run.
    pub fn push_all(&mut self, cx: &mut Context<Self>) {
        if self.is_syncing() {
            return;
        }
        // The rows show one batch's verdicts at a time.
        self.sync = None;
        self.sync_task = None;

        let mut prefailed = Vec::new();
        let mut entries = Vec::new();
        for repo in &self.state.repos {
            let Some(clone) = self.local_path(&repo.id) else {
                continue;
            };
            for pr in &repo.prs {
                let key = (repo.id.clone(), pr.number);
                match BranchName::new(pr.head_ref.as_str()) {
                    Ok(branch) => entries.push((
                        key,
                        PushTarget {
                            clone: clone.clone(),
                            branch,
                            head_repo: pr.head_repository_in(&repo.id),
                        },
                    )),
                    Err(err) => prefailed.push((key, PushResult::Failed(err.to_string()))),
                }
            }
        }

        tracing::info!(branches = entries.len(), "planning a push all");
        self.push_all.state = Some(PushAll::Planning {
            total: entries.len() + prefailed.len(),
        });
        cx.notify();

        self.push_all.task = Some(cx.spawn(async move |this, cx| {
            let planned = Tokio::spawn(&*cx, plan_push_all(entries)).await;
            this.update(cx, |this, cx| match planned {
                Ok(plan) if plan.needs_confirmation() => {
                    this.push_all.state = Some(PushAll::Confirming { plan, prefailed });
                    cx.notify();
                }
                // Nothing to rewrite: there is nothing to ask.
                Ok(plan) => this.run_push_steps(
                    plan.authorize(RewriteDecision::FastForwardsOnly),
                    prefailed,
                    cx,
                ),
                Err(err) => {
                    tracing::warn!(%err, "push all planning did not complete");
                    this.push_all.clear();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The user's answer to the confirmation: `None` cancels the batch and
    /// pushes nothing.
    pub fn answer_push_all(&mut self, decision: Option<RewriteDecision>, cx: &mut Context<Self>) {
        let Some(PushAll::Confirming { .. }) = &self.push_all.state else {
            return;
        };
        let Some(PushAll::Confirming { plan, prefailed }) = self.push_all.state.take() else {
            return;
        };
        match decision {
            None => {
                tracing::info!("push all cancelled at confirmation");
                self.push_all.clear();
                cx.notify();
            }
            Some(decision) => {
                tracing::info!(
                    ?decision,
                    rewrites = plan.rewrites().len(),
                    "push all confirmed"
                );
                self.run_push_steps(plan.authorize(decision), prefailed, cx);
            }
        }
    }

    fn run_push_steps(
        &mut self,
        steps: Vec<PushStep<PushKey>>,
        prefailed: Vec<(PushKey, PushResult)>,
        cx: &mut Context<Self>,
    ) {
        let mut results: BTreeMap<PushKey, PushResult> = prefailed.into_iter().collect();
        let mut runs = Vec::new();
        for step in steps {
            match step {
                PushStep::Settled { key, result } => {
                    results.insert(key, result);
                }
                PushStep::Run {
                    key,
                    target,
                    confirmation,
                } => runs.push((key, target, confirmation)),
            }
        }
        let done = results.len();
        self.push_all.state = Some(PushAll::Running(PushProgress {
            done,
            total: done + runs.len(),
            results,
        }));
        cx.notify();

        self.push_all.task = Some(cx.spawn(async move |this, cx| {
            let mut landed = BTreeSet::new();
            for (key, target, confirmation) in runs {
                let outcome = Tokio::spawn(&*cx, async move {
                    run_push(&target, confirmation.as_ref()).await
                })
                .await;
                let result = match outcome {
                    Ok(result) => result,
                    Err(err) => PushResult::Failed(format!("push did not complete: {err}")),
                };
                if result.landed() {
                    landed.insert(key.0.clone());
                }
                if this
                    .update(cx, |this, cx| this.record_push_result(key, result, cx))
                    .is_err()
                {
                    return;
                }
            }
            // GitHub's view of every repository that received a push.
            this.update(cx, |this, cx| {
                for repo in landed {
                    this.refresh_repo(repo, cx);
                }
            })
            .ok();
        }));
    }

    fn record_push_result(&mut self, key: PushKey, result: PushResult, cx: &mut Context<Self>) {
        let Some(PushAll::Running(progress)) = &mut self.push_all.state else {
            return;
        };
        if self.state.repos.iter().any(|repo| repo.id == key.0) {
            tracing::debug!(repo = %key.0, number = key.1.0, outcome = %result.detail(), "push all entry finished");
            progress.results.insert(key, result);
        }
        progress.done += 1;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use rostrum_git::{Oid, PushRejection, RemoteRef};

    use super::*;

    fn upstream() -> RemoteRef {
        RemoteRef::origin(BranchName::new("x").expect("valid"))
    }

    #[test]
    fn every_reportable_push_outcome_lands_in_its_own_bucket() {
        let oid = Oid::parse("a".repeat(40)).expect("valid");
        let results = [
            PushResult::Pushed {
                upstream: upstream(),
                rewrote: false,
            },
            PushResult::Pushed {
                upstream: upstream(),
                rewrote: true,
            },
            PushResult::Skipped(PushSkip::NoUpstream),
            PushResult::Skipped(PushSkip::UpToDate),
            PushResult::Skipped(PushSkip::NotCheckedOut),
            PushResult::NotConfirmed {
                expected: oid.clone(),
            },
            PushResult::ConfirmationStale {
                confirmed: oid.clone(),
                current: oid,
            },
            PushResult::Rejected(PushRejection::StaleLease),
            PushResult::Failed("f".into()),
        ];
        assert_eq!(
            summarise(results.iter()),
            PushSummary {
                pushed: 1,
                rewritten: 1,
                skipped: 1,
                not_confirmed: 1,
                refused: 2,
                failed: 1,
            }
        );
        assert_eq!(
            summarise(results.iter()).describe(),
            "1 pushed, 1 rewritten, 1 skipped, 1 not confirmed, 2 refused, 1 failed"
        );
        assert_eq!(PushSummary::default().describe(), "nothing to push");
    }

    #[test]
    fn a_push_all_is_active_until_its_run_finishes() {
        let mut sync = PushAllSync::default();
        assert!(!sync.is_active());
        sync.state = Some(PushAll::Planning { total: 2 });
        assert!(sync.is_active());
        sync.state = Some(PushAll::Running(PushProgress {
            done: 1,
            total: 2,
            results: BTreeMap::new(),
        }));
        assert!(sync.is_active());
        sync.state = Some(PushAll::Running(PushProgress {
            done: 2,
            total: 2,
            results: BTreeMap::new(),
        }));
        assert!(!sync.is_active());
    }
}
