//! The store's stack half: GitHub's stacks for each repository, kept fresh
//! beside the pull requests, and the one stack operation that may run at a
//! time — make, arrange, merge or unstack.
//!
//! The operations themselves are `rostrum-stack`'s; this module only starts
//! them on the Tokio side, relays their progress, and refreshes the
//! repository when they finish so the feed shows what GitHub now says.

use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    time::{Duration, Instant},
};

use futures::StreamExt;
use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{RepoId, Stack, StackNumber, StackPlan};
use rostrum_github::{GitHubError, RepoStacks};
use rostrum_stack::{
    GhCli, MergeMethod, Progress, StackJob, StackOutcome, StackProgress, merge_stack,
    run_stack_job, unstack,
};

use super::Store;

/// How long to wait before asking a repository without stacked pull requests
/// enabled again. Long, because the answer rarely changes, and short enough
/// that enabling it does not need a restart.
const UNAVAILABLE_BACKOFF: Duration = Duration::from_secs(60 * 60);

/// The stack state the store holds.
#[derive(Default)]
pub struct StackSync {
    /// In-flight Stacks API read per repository; replacing one cancels it.
    fetches: HashMap<RepoId, Task<()>>,
    /// Repositories that answered 404, and when.
    unavailable: HashMap<RepoId, Instant>,
    op: Option<StackOpStatus>,
    /// Drives the running operation. Dropping it abandons waiting for the
    /// result, never the operation: that runs to completion on Tokio.
    task: Option<Task<()>>,
    progress_task: Option<Task<()>>,
}

impl StackSync {
    pub fn forget(&mut self, repo: &RepoId) {
        self.fetches.remove(repo);
        self.unavailable.remove(repo);
    }
}

/// Which stack operation is running or last ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackOpKind {
    Make,
    Arrange,
    Merge,
    Unstack,
}

impl StackOpKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Make => "Make stack",
            Self::Arrange => "Arrange",
            Self::Merge => "Merge stack",
            Self::Unstack => "Unstack",
        }
    }
}

/// How an operation ended, for the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackOpResult {
    Succeeded(String),
    /// It stopped somewhere that needs the user: a conflict (aborted or
    /// handed off), a refused lease, or a link that failed after pushing.
    Stopped(String),
    /// It could not run, and nothing changed.
    Failed(String),
}

/// The operation running now, or the last one, until dismissed.
#[derive(Clone, Debug)]
pub struct StackOpStatus {
    pub repo: RepoId,
    pub kind: StackOpKind,
    pub progress: Option<StackProgress>,
    pub finished: Option<StackOpResult>,
}

impl StackOpStatus {
    pub fn line(&self) -> String {
        match (&self.finished, &self.progress) {
            (Some(StackOpResult::Succeeded(text)), _)
            | (Some(StackOpResult::Stopped(text)), _)
            | (Some(StackOpResult::Failed(text)), _) => {
                format!("{} ({}): {text}", self.kind.label(), self.repo)
            }
            (None, Some(progress)) => format!("{}: {}", self.kind.label(), progress.describe()),
            (None, None) => format!("{}: starting…", self.kind.label()),
        }
    }
}

/// Where scratch worktrees for arranging go, and where a merge without a
/// clone runs from.
fn scratch_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("rostrum")
        .join("stack-worktrees")
}

impl Store {
    /// Ask GitHub's Stacks API for the repository's stacks, after a refresh.
    ///
    /// Skipped for a repository with no open pull requests (there is nothing
    /// to group) and, for an hour, for one that said stacks are not enabled.
    pub(super) fn fetch_stacks(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        if self.state.repo(id).is_none_or(|repo| repo.prs.is_empty()) {
            return;
        }
        if self
            .stacks
            .unavailable
            .get(id)
            .is_some_and(|since| since.elapsed() < UNAVAILABLE_BACKOFF)
        {
            return;
        }

        let fetch_id = id.clone();
        let apply_id = id.clone();
        let task = cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(&*cx, async move { client.stacks(&fetch_id).await }).await;
            this.update(cx, |this, cx| this.apply_stacks(&apply_id, outcome, cx))
                .ok();
        });
        self.stacks.fetches.insert(id.clone(), task);
    }

    fn apply_stacks(
        &mut self,
        id: &RepoId,
        outcome: Result<Result<RepoStacks, GitHubError>, gpui_tokio::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let stacks = match outcome {
            Ok(Ok(RepoStacks::Available(stacks))) => {
                self.stacks.unavailable.remove(id);
                stacks
            }
            Ok(Ok(RepoStacks::Unavailable)) => {
                tracing::debug!(repo = %id, "stacked pull requests are not enabled");
                self.stacks.unavailable.insert(id.clone(), Instant::now());
                Vec::new()
            }
            // A failed read keeps what was known: a stack does not stop
            // being one because one poll could not ask.
            Ok(Err(error)) => {
                tracing::debug!(repo = %id, %error, "stacks read failed; keeping the last answer");
                self.stacks.fetches.remove(id);
                return;
            }
            Err(error) => {
                tracing::debug!(repo = %id, %error, "stacks read did not complete");
                self.stacks.fetches.remove(id);
                return;
            }
        };

        if let Some(repo) = self.state.repo_mut(id)
            && repo.stacks != stacks
        {
            tracing::debug!(repo = %id, count = stacks.len(), "stacks updated");
            repo.stacks = stacks.clone();
            cx.notify();
        }
        if let Some(db) = self.db.clone() {
            let repo = id.clone();
            Tokio::spawn(&*cx, async move {
                if let Err(error) = db.save_stacks(&repo, &stacks).await {
                    tracing::warn!(%repo, %error, "could not cache stacks");
                }
            })
            .detach();
        }
        self.stacks.fetches.remove(id);
    }

    /// Put cached stacks in place on a cold start, never over fresher ones.
    pub(super) fn hydrate_stacks(&mut self, id: &RepoId, stacks: Vec<Stack>) {
        if let Some(repo) = self.state.repo_mut(id)
            && repo.stacks.is_empty()
            && !stacks.is_empty()
        {
            tracing::debug!(repo = %id, count = stacks.len(), "restored stacks from cache");
            repo.stacks = stacks;
        }
    }

    pub fn stack_op(&self) -> Option<&StackOpStatus> {
        self.stacks.op.as_ref()
    }

    pub fn is_stack_busy(&self) -> bool {
        self.stacks
            .op
            .as_ref()
            .is_some_and(|op| op.finished.is_none())
    }

    pub fn dismiss_stack_op(&mut self, cx: &mut Context<Self>) {
        if !self.is_stack_busy() {
            self.stacks.op = None;
            cx.notify();
        }
    }

    /// Make `plan` into a stack from the repository's clone: "Make stack"
    /// when its bases already chain, "Arrange" when branches must be rebased
    /// and force-pushed (with a lease) first.
    pub fn make_stack(&mut self, plan: StackPlan, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(clone) = self.local_path(&plan.repo) else {
            return Err(format!(
                "{} has no local clone configured; stacks are made from a clone",
                plan.repo
            ));
        };
        let kind = if plan.needs_rewrite() {
            StackOpKind::Arrange
        } else {
            StackOpKind::Make
        };
        let handler = self.conflict_handler();
        let repo = plan.repo.clone();
        self.run_stack_op(repo, kind, cx, move |progress| async move {
            let job = StackJob {
                clone,
                plan,
                handler,
                scratch_dir: scratch_dir(),
            };
            match run_stack_job(job, &GhCli, &progress).await {
                Ok(outcome @ StackOutcome::Stacked(_)) => {
                    StackOpResult::Succeeded(outcome.summary())
                }
                Ok(outcome) => StackOpResult::Stopped(outcome.summary()),
                Err(error) => StackOpResult::Failed(error.to_string()),
            }
        })
    }

    /// GitHub's all-or-nothing merge of stack `number`.
    pub fn merge_stack(
        &mut self,
        repo: RepoId,
        number: StackNumber,
        method: MergeMethod,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let cwd = self.local_path(&repo).unwrap_or_else(scratch_dir);
        let target = repo.clone();
        self.run_stack_op(repo, StackOpKind::Merge, cx, move |progress| async move {
            if let Err(error) = std::fs::create_dir_all(&cwd) {
                return StackOpResult::Failed(format!(
                    "could not prepare {}: {error}",
                    cwd.display()
                ));
            }
            match merge_stack(&GhCli, &cwd, &target, number, method, &progress).await {
                Ok(message) if message.is_empty() => {
                    StackOpResult::Succeeded(format!("stack {number} merged ({})", method.label()))
                }
                Ok(message) => StackOpResult::Succeeded(message),
                Err(error) => StackOpResult::Failed(error.to_string()),
            }
        })
    }

    /// Dissolve stack `number` on GitHub (and in the clone, if tracked).
    pub fn unstack(
        &mut self,
        repo: RepoId,
        number: StackNumber,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(clone) = self.local_path(&repo) else {
            return Err(format!(
                "{repo} has no local clone configured; `gh stack unstack` runs in one"
            ));
        };
        let target = repo.clone();
        self.run_stack_op(repo, StackOpKind::Unstack, cx, move |progress| async move {
            match unstack(&GhCli, &clone, &target, number, &progress).await {
                Ok(message) if message.is_empty() => {
                    StackOpResult::Succeeded(format!("stack {number} unstacked"))
                }
                Ok(message) => StackOpResult::Succeeded(message),
                Err(error) => StackOpResult::Failed(error.to_string()),
            }
        })
    }

    /// Start one operation: progress flows back over a channel, the result
    /// lands on the status, and the repository is refreshed so the feed
    /// shows what GitHub now says.
    fn run_stack_op<F, Fut>(
        &mut self,
        repo: RepoId,
        kind: StackOpKind,
        cx: &mut Context<Self>,
        operation: F,
    ) -> Result<(), String>
    where
        F: FnOnce(Progress) -> Fut,
        Fut: Future<Output = StackOpResult> + Send + 'static,
    {
        if self.is_stack_busy() {
            return Err("another stack operation is still running".into());
        }
        let (sender, mut receiver) = futures::channel::mpsc::unbounded();
        let future = operation(Progress::new(sender));
        tracing::info!(%repo, kind = kind.label(), "stack operation started");
        self.stacks.op = Some(StackOpStatus {
            repo: repo.clone(),
            kind,
            progress: None,
            finished: None,
        });
        cx.notify();

        self.stacks.progress_task = Some(cx.spawn(async move |this, cx| {
            while let Some(progress) = receiver.next().await {
                let updated = this.update(cx, |this, cx| {
                    if let Some(op) = &mut this.stacks.op
                        && op.finished.is_none()
                    {
                        op.progress = Some(progress);
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        }));

        self.stacks.task = Some(cx.spawn(async move |this, cx| {
            let result = match Tokio::spawn(&*cx, future).await {
                Ok(result) => result,
                Err(error) => StackOpResult::Failed(format!("did not complete: {error}")),
            };
            this.update(cx, |this, cx| {
                tracing::info!(%repo, kind = kind.label(), ?result, "stack operation finished");
                if let Some(op) = &mut this.stacks.op {
                    op.finished = Some(result);
                }
                // Let any read that started before the operation finish
                // first; the refresh it triggers re-reads the stacks too.
                this.stacks.fetches.remove(&repo);
                this.refresh_repo(repo.clone(), cx);
                cx.notify();
            })
            .ok();
        }));
        Ok(())
    }
}
