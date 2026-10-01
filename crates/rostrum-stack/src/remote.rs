//! The two stack actions that act on GitHub alone: merging and unstacking.

use std::path::Path;

use rostrum_core::{RepoId, StackNumber};

use crate::{
    error::StackOpError,
    gh::{GhRunner, GhStackCommand, MergeMethod},
    job::{Progress, StackProgress},
};

/// Merge every member of stack `stack` into its trunk with GitHub's atomic
/// stack merge: all of them or none.
///
/// `cwd` is any existing directory: `merge <number>` reads nothing local, so
/// it works for a repository without a clone. Returns `gh`'s own account of
/// what happened. GitHub evaluates branch protection when the merge runs; a
/// refusal comes back as [`StackOpError::GhFailed`] with its reason, and
/// nothing was merged.
pub async fn merge_stack<R: GhRunner>(
    gh: &R,
    cwd: &Path,
    repo: &RepoId,
    stack: StackNumber,
    method: MergeMethod,
    progress: &Progress,
) -> Result<String, StackOpError> {
    progress.send(StackProgress::Merging);
    let command = GhStackCommand::Merge { stack, method };
    let output = gh
        .run(cwd, repo, &command)
        .await?
        .require_success(&command)?;
    tracing::info!(%repo, stack = stack.get(), method = method.as_flag_value(), "stack merged");
    Ok(output.message())
}

/// Dissolve stack `stack` on GitHub, and in the clone if gh-stack tracks it
/// there. The pull requests stay open with the bases they have.
///
/// Needs the clone: `gh stack unstack` looks the number up in the local
/// tracking file before going to GitHub, and fails outside a repository.
pub async fn unstack<R: GhRunner>(
    gh: &R,
    clone: &Path,
    repo: &RepoId,
    stack: StackNumber,
    progress: &Progress,
) -> Result<String, StackOpError> {
    progress.send(StackProgress::Unstacking);
    let command = GhStackCommand::Unstack { stack };
    let output = gh
        .run(clone, repo, &command)
        .await?
        .require_success(&command)?;
    tracing::info!(%repo, stack = stack.get(), "stack unstacked");
    Ok(output.message())
}
