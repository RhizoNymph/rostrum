//! The stack operations, behind a seam tests can replace.
//!
//! `rostrum-stack`'s functions are generic over a `GhRunner` (an
//! `impl Future` trait, not object-safe), so the daemon holds them as an
//! `Arc<dyn StackOps>`: [`GhStackOps`] runs the real functions with any
//! runner — `GhCli` in the service, a recording double in tests that run
//! real git against scratch repositories.

use std::{path::PathBuf, sync::Arc};

use rostrum_core::{RepoId, StackNumber};
use rostrum_stack::{
    ExtendJob, GhRunner, MergeMethod, Progress, StackJob, StackOpError, StackOutcome,
};

use crate::boxed::BoxFuture;

pub trait StackOps: Send + Sync {
    fn make(
        &self,
        job: StackJob,
        progress: Progress,
    ) -> BoxFuture<'_, Result<StackOutcome, StackOpError>>;

    fn extend(
        &self,
        job: ExtendJob,
        progress: Progress,
    ) -> BoxFuture<'_, Result<StackOutcome, StackOpError>>;

    fn merge(
        &self,
        cwd: PathBuf,
        repo: RepoId,
        stack: StackNumber,
        method: MergeMethod,
        progress: Progress,
    ) -> BoxFuture<'_, Result<String, StackOpError>>;

    fn unstack(
        &self,
        clone: PathBuf,
        repo: RepoId,
        stack: StackNumber,
        progress: Progress,
    ) -> BoxFuture<'_, Result<String, StackOpError>>;
}

/// `rostrum-stack` itself, over `gh` runner `R`.
pub struct GhStackOps<R>(pub Arc<R>);

impl<R: GhRunner + 'static> StackOps for GhStackOps<R> {
    fn make(
        &self,
        job: StackJob,
        progress: Progress,
    ) -> BoxFuture<'_, Result<StackOutcome, StackOpError>> {
        Box::pin(async move { rostrum_stack::run_stack_job(job, self.0.as_ref(), &progress).await })
    }

    fn extend(
        &self,
        job: ExtendJob,
        progress: Progress,
    ) -> BoxFuture<'_, Result<StackOutcome, StackOpError>> {
        Box::pin(
            async move { rostrum_stack::run_extend_job(job, self.0.as_ref(), &progress).await },
        )
    }

    fn merge(
        &self,
        cwd: PathBuf,
        repo: RepoId,
        stack: StackNumber,
        method: MergeMethod,
        progress: Progress,
    ) -> BoxFuture<'_, Result<String, StackOpError>> {
        Box::pin(async move {
            rostrum_stack::merge_stack(self.0.as_ref(), &cwd, &repo, stack, method, &progress).await
        })
    }

    fn unstack(
        &self,
        clone: PathBuf,
        repo: RepoId,
        stack: StackNumber,
        progress: Progress,
    ) -> BoxFuture<'_, Result<String, StackOpError>> {
        Box::pin(async move {
            rostrum_stack::unstack(self.0.as_ref(), &clone, &repo, stack, &progress).await
        })
    }
}

/// The phone's merge method as `gh stack merge` takes it.
pub fn merge_method(method: rostrum_remote::StackMergeMethod) -> MergeMethod {
    match method {
        rostrum_remote::StackMergeMethod::Merge => MergeMethod::Merge,
        rostrum_remote::StackMergeMethod::Squash => MergeMethod::Squash,
        rostrum_remote::StackMergeMethod::Rebase => MergeMethod::Rebase,
    }
}

#[cfg(test)]
mod tests {
    use rostrum_remote::StackMergeMethod;

    use super::*;

    #[test]
    fn merge_methods_map_one_to_one() {
        assert_eq!(merge_method(StackMergeMethod::Merge), MergeMethod::Merge);
        assert_eq!(merge_method(StackMergeMethod::Squash), MergeMethod::Squash);
        assert_eq!(merge_method(StackMergeMethod::Rebase), MergeMethod::Rebase);
    }
}
