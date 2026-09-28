//! Where a pull request's branch is checked out, and how far it has drifted.

use std::{path::Path, path::PathBuf, time::Duration};

use rostrum_core::Divergence;
use rostrum_git::{Autostash, BranchName, GitError, InProgress, Operation, RemoteRef, Repo, Rev};
use rostrum_handoff::session_exists;

/// How long to give tmux to answer whether a session exists.
const TMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// The local half of a pull request whose repository has a clone configured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalState {
    /// The clone exists but no worktree has this branch checked out. Common
    /// in a one-worktree-per-branch layout for pull requests the user is not
    /// working on, so it is a quiet line rather than a failure.
    NotCheckedOut,
    CheckedOut(LocalBranch),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBranch {
    /// The worktree this branch is checked out in — not necessarily the
    /// configured clone path, which may be any worktree of the repository.
    pub worktree: PathBuf,
    pub branch: BranchName,
    pub remote: RemoteRef,
    /// The local branch measured against its remote counterpart: `ahead` is
    /// work not pushed yet, `behind` is work not pulled yet.
    pub divergence: Divergence,
    /// Whether the refs these counts came from were refreshed just now. A fetch
    /// that failed leaves real numbers computed from stale refs, which is worth
    /// saying out loud rather than presenting as current.
    pub fetched: bool,
    /// Why a local action cannot run, if anything is in the way.
    pub blocker: Option<String>,
    /// A rebase or merge git has started and not finished in this worktree.
    pub in_progress: Option<InProgress>,
    /// When something is in progress and a handler is configured: whether the
    /// tmux session that was (or would have been) handed the conflict exists.
    pub handoff: Option<HandoffState>,
}

/// Whether a handed-off conflict still has someone working on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandoffState {
    Running {
        session: String,
    },
    /// The worktree is mid-operation but no session by the expected name
    /// exists — the harness finished without continuing, or was killed.
    Gone {
        session: String,
    },
}

/// Find the worktree `branch` is checked out in under `clone`, fetch its
/// remote counterpart, and measure the drift.
///
/// `handoff_session` is the tmux session a conflict on this pull request would
/// be handed to, present exactly when a conflict handler is configured; it is
/// only consulted when something is in progress.
///
/// The fetch is allowed to fail. Its only job is to make the remote-tracking
/// ref current; when it cannot — no network, a locked credential — the counts
/// are still computed, still truthful about what is on disk, and flagged as
/// unfetched.
pub async fn local_state(
    clone: &Path,
    branch: BranchName,
    autostash: Autostash,
    handoff_session: Option<String>,
) -> Result<LocalState, GitError> {
    let clone = Repo::open(clone).await?;
    let Some(repo) = clone.worktree_for(&branch).await? else {
        return Ok(LocalState::NotCheckedOut);
    };
    let remote = RemoteRef::origin(branch.clone());

    let fetched = match repo.fetch(&remote).await {
        Ok(outcome) => {
            tracing::debug!(?outcome, "fetched the pull request branch");
            true
        }
        Err(error) => {
            tracing::debug!(%error, "could not fetch; using the refs already on disk");
            false
        }
    };

    let divergence = repo
        .divergence(&Rev::Local(branch.clone()), &Rev::Remote(remote.clone()))
        .await?;

    let status = repo.status().await?;
    let in_progress = status.in_progress;

    let blocker = repo
        .preflight(Operation::PullRebase, Some(&branch), autostash)
        .await?
        .reason();

    // Only worth asking tmux when there is something a session could be
    // working on. An error here degrades to "unknown" rather than failing a
    // state that is otherwise fine.
    let handoff = match (in_progress, handoff_session) {
        (Some(_), Some(session)) => match session_exists(&session, TMUX_TIMEOUT).await {
            Ok(true) => Some(HandoffState::Running { session }),
            Ok(false) => Some(HandoffState::Gone { session }),
            Err(error) => {
                tracing::warn!(%error, "could not ask tmux about the handoff session");
                None
            }
        },
        _ => None,
    };

    Ok(LocalState::CheckedOut(LocalBranch {
        worktree: repo.root().to_path_buf(),
        branch,
        remote,
        divergence,
        fetched,
        blocker,
        in_progress,
        handoff,
    }))
}

/// Abort whatever rebase or merge is in progress in `worktree`.
///
/// The target comes from the worktree's own state via
/// [`InProgress::abort_target`], so it cannot run `merge --abort` on a rebase.
pub async fn abort_in_progress(worktree: &Path) -> Result<(), GitError> {
    let repo = Repo::open(worktree).await?;
    let status = repo.status().await?;
    let Some(target) = status.in_progress.and_then(InProgress::abort_target) else {
        return Err(GitError::NothingToDescribe {
            in_progress: status.in_progress,
        });
    };
    repo.abort(target).await
}
