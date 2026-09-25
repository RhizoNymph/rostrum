//! One local git operation on one pull request, from clone path to verdict.
//!
//! The detail pane's buttons and the feed's "sync all" run the same thing on
//! the same terms, so the whole sequence lives here rather than in either view:
//! find the worktree the branch is checked out in, run the operation, and —
//! when the user has configured a conflict handler — hand a stopped rebase or
//! merge to it instead of aborting.
//!
//! Nothing here touches GPUI. It is `async` code meant to run inside a
//! `Tokio::spawn`, returning a plain [`LocalResult`] the caller renders.

use std::{path::PathBuf, time::Duration};

use rostrum_git::{Autostash, BranchName, ConflictPolicy, GitError, Outcome, RemoteRef, Repo};
use rostrum_handoff::{HandoffError, PrMeta, Spawned, hand_off, session_exists, session_name};

use crate::config::ConflictHandler;

/// How long to give the tmux client. It relays argv to a server and exits;
/// anything slower than this is a wedged server, not a slow one.
const TMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// Which local operation to run, and against which ref.
///
/// Two targets: the branch's remote counterpart (`origin/<head>`, the pull
/// family) and its base (`origin/<base>`, the update family). Naming both in
/// the variant keeps a caller from merging the wrong one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalOp {
    /// Fetch `origin/<head>` and rebase the local commits on top of it.
    PullRebase,
    /// Merge `origin/<head>` into the local branch.
    MergeRemote,
    /// Merge `origin/<base>` into the local branch.
    MergeBase,
    /// Rebase the local branch onto `origin/<base>`.
    RebaseBase,
}

impl LocalOp {
    /// Label for the in-flight banner.
    pub fn progress_label(self) -> &'static str {
        match self {
            Self::PullRebase => "Pulling",
            Self::MergeRemote => "Merging remote branch",
            Self::MergeBase => "Merging base locally",
            Self::RebaseBase => "Rebasing onto base locally",
        }
    }

    fn targets_base(self) -> bool {
        matches!(self, Self::MergeBase | Self::RebaseBase)
    }
}

/// Everything one run needs, gathered on the main thread before spawning.
#[derive(Clone, Debug)]
pub struct LocalJob {
    /// Any worktree of the clone; the one for `branch` is found from it.
    pub clone: PathBuf,
    pub branch: BranchName,
    pub base: BranchName,
    pub op: LocalOp,
    pub autostash: Autostash,
    /// Present ⇒ a conflict is left in place and handed off. Absent ⇒ aborted.
    pub handler: Option<ConflictHandler>,
    pub pr: PrMeta,
}

/// What happened, in terms a view can render directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalResult {
    /// The branch is not checked out in any worktree of the clone.
    NotCheckedOut,
    UpToDate,
    Completed,
    /// git declined to start; the reason is git's own. Repository untouched.
    Refused(String),
    /// Stopped on a conflict and aborted — no handler, or the handler could
    /// not be started. Repository as it was found.
    Conflicted(String),
    /// Stopped on a conflict and left for the named tmux session.
    HandedOff {
        session: String,
    },
    /// Something other than git refused: the clone could not be opened, the
    /// operation timed out, or a handler was configured but its command
    /// template is unusable.
    Failed(String),
}

impl LocalResult {
    /// Short chip text for the feed, or `None` for the unremarkable outcomes.
    pub fn chip(&self) -> Option<&'static str> {
        match self {
            Self::NotCheckedOut | Self::UpToDate | Self::Completed => None,
            Self::Refused(_) => Some("refused"),
            Self::Conflicted(_) => Some("conflict"),
            Self::HandedOff { .. } => Some("handed off"),
            Self::Failed(_) => Some("failed"),
        }
    }

    /// The sentence behind the chip, or for the banner.
    pub fn detail(&self) -> String {
        match self {
            Self::NotCheckedOut => "not checked out in any worktree".into(),
            Self::UpToDate => "already up to date".into(),
            Self::Completed => "updated".into(),
            Self::Refused(reason) | Self::Conflicted(reason) | Self::Failed(reason) => {
                reason.clone()
            }
            Self::HandedOff { session } => {
                format!("handed off to tmux session `{session}` — tmux attach -t ={session}")
            }
        }
    }
}

/// Run one job to completion.
///
/// Every failure is a value, never a panic and never an `Err`: a job runs as
/// one of many in a sync, and one bad clone must not stop the rest.
pub async fn run_local_job(job: LocalJob) -> LocalResult {
    match run(job).await {
        Ok(result) => result,
        Err(err) => LocalResult::Failed(err.to_string()),
    }
}

async fn run(job: LocalJob) -> Result<LocalResult, GitError> {
    // A misconfigured handler is a click-time error, found before any git
    // write. So is a session already running for this pull request: typing a
    // second command into it would interleave with whatever it is doing.
    if let Some(handler) = &job.handler {
        let session = session_name(&job.pr.repo, job.pr.number);
        if let Err(err) = preflight_handler(handler, &session).await {
            return Ok(LocalResult::Failed(err));
        }
    }

    let policy = match job.handler {
        Some(_) => ConflictPolicy::Leave,
        None => ConflictPolicy::Abort,
    };
    let clone = Repo::open(&job.clone).await?.with_conflict_policy(policy);
    let Some(repo) = clone.worktree_for(&job.branch).await? else {
        return Ok(LocalResult::NotCheckedOut);
    };

    let target = if job.op.targets_base() {
        RemoteRef::origin(job.base.clone())
    } else {
        RemoteRef::origin(job.branch.clone())
    };

    let outcome = match job.op {
        LocalOp::PullRebase => repo.pull_rebase(&target, job.autostash).await,
        LocalOp::MergeRemote | LocalOp::MergeBase => {
            repo.merge_from(&job.branch, &target, job.autostash).await
        }
        LocalOp::RebaseBase => repo.rebase_onto(&job.branch, &target, job.autostash).await,
    };

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(GitError::Refused { blocker, .. }) => {
            return Ok(LocalResult::Refused(blocker.reason()));
        }
        Err(err) => return Err(err),
    };

    match outcome {
        Outcome::AlreadyUpToDate => Ok(LocalResult::UpToDate),
        Outcome::Completed { .. } => Ok(LocalResult::Completed),
        Outcome::Conflicted(conflict) => {
            let Some(abort_target) = conflict.abort_target() else {
                // Already aborted under `Abort`, or an autostash-pop conflict
                // with nothing to abort. Either way there is nothing to hand
                // off: report git's words.
                return Ok(LocalResult::Conflicted(conflict.message().to_string()));
            };
            let Some(handler) = &job.handler else {
                // Unreachable under `Abort`, but the type does not know that.
                return Ok(LocalResult::Conflicted(conflict.message().to_string()));
            };

            // Gather, write, spawn. Any failure before the session exists
            // means nobody is going to finish this rebase, so it is aborted —
            // the same guarantee the no-handler path gives.
            let mut context = match repo.conflict_context(&job.branch, &target).await {
                Ok(context) => context,
                Err(err) => {
                    let _ = repo.abort(abort_target).await;
                    return Ok(LocalResult::Conflicted(format!(
                        "{}; could not gather context for the handler ({err}), so the operation was aborted",
                        conflict.message()
                    )));
                }
            };
            context.git_message = conflict.message().to_string();

            match hand_off(
                &job.pr,
                &context,
                repo.root(),
                &handler.command,
                TMUX_TIMEOUT,
            )
            .await
            {
                Ok(receipt) => {
                    if receipt.spawned == Spawned::AlreadyRunning {
                        tracing::info!(session = %receipt.session, "handoff session was already running");
                    }
                    Ok(LocalResult::HandedOff {
                        session: receipt.session,
                    })
                }
                Err(err) => {
                    let _ = repo.abort(abort_target).await;
                    Ok(LocalResult::Conflicted(format!(
                        "{}; the conflict handler could not be started ({err}), so the operation was aborted",
                        conflict.message()
                    )))
                }
            }
        }
    }
}

/// Everything about a handler that can be checked before git runs.
async fn preflight_handler(handler: &ConflictHandler, session: &str) -> Result<(), String> {
    // The real paths are not known yet; any two will do to prove the template
    // mentions `{context}`.
    let probe = std::path::Path::new("/probe");
    rostrum_handoff::substitute(&handler.command, probe, probe).map_err(|err| err.to_string())?;

    match session_exists(session, TMUX_TIMEOUT).await {
        Ok(true) => Err(format!(
            "a handoff for this pull request is already running in tmux session `{session}` — finish or kill it first"
        )),
        Ok(false) => Ok(()),
        Err(HandoffError::TmuxMissing { .. }) => {
            Err("a conflict handler is configured but `tmux` is not installed".into())
        }
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_remarkable_outcomes_get_a_chip() {
        assert_eq!(LocalResult::UpToDate.chip(), None);
        assert_eq!(LocalResult::Completed.chip(), None);
        assert_eq!(LocalResult::NotCheckedOut.chip(), None);
        assert_eq!(LocalResult::Refused("x".into()).chip(), Some("refused"));
        assert_eq!(LocalResult::Conflicted("x".into()).chip(), Some("conflict"));
        assert_eq!(
            LocalResult::HandedOff {
                session: "s".into()
            }
            .chip(),
            Some("handed off")
        );
        assert_eq!(LocalResult::Failed("x".into()).chip(), Some("failed"));
    }

    /// The base family targets `origin/<base>`; the pull family targets
    /// `origin/<head>`. Getting this backwards would merge main into itself.
    #[test]
    fn the_update_family_targets_the_base_and_the_pull_family_the_remote() {
        assert!(LocalOp::MergeBase.targets_base());
        assert!(LocalOp::RebaseBase.targets_base());
        assert!(!LocalOp::PullRebase.targets_base());
        assert!(!LocalOp::MergeRemote.targets_base());
    }

    #[test]
    fn a_handed_off_result_tells_the_user_how_to_attach() {
        let detail = LocalResult::HandedOff {
            session: "rostrum-a-b-1".into(),
        }
        .detail();
        assert!(detail.contains("tmux attach -t =rostrum-a-b-1"));
    }
}
