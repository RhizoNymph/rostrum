//! Stack actions from the phone, run by the paired desktop.
//!
//! The phone cannot run `gh` or git, so every action is a request to
//! `rostrumd` (`rostrum-remote`'s stack routes): the desktop re-validates it
//! against GitHub as it is now and runs `rostrum-stack` on its clone. Long
//! operations are jobs: each call answers at once with a [`StackJob`], and
//! `stack_job` polls it until `finished`. Once a job is seen finished, the
//! repository's feed is refreshed (once per job), so the observer shows the
//! new stack, merge or rewrite.
//!
//! Rewriting history is never implicit: `arrange_stack` and `extend_stack`
//! carry `confirm_rewrite`, exactly the branches `plan_stack_rewrite` named.
//! A mismatch comes back as [`RostrumError::RewriteNotConfirmed`] carrying the
//! branches the desktop would actually rewrite (re-asked from its dry run),
//! so the phone can show them and ask again. A desktop busy with another job
//! on the same clone answers `RemoteApi` with code `BUSY`, as for local jobs.
//!
//! Cheap checks happen here first — numbers listed once, enough of them, a
//! valid trunk and stack number — and `check_stack_plan` / `stack_candidates`
//! answer from the cached feed with `rostrum-core`'s own planners, so the
//! phone can show what is eligible before asking the desktop.

mod check;
mod types;

use std::sync::Arc;

use rostrum_core::{RepoId, RepoState};
use rostrum_remote::{
    ArrangeStackRequest, ExtendStackRequest, MakeStackRequest, MergeStackRequest, StackJobId,
    StackJobStatus, UnstackRequest,
    api::ApiErrorCode,
    client::{ClientError, RemoteClient},
};

pub use types::{
    StackCandidate, StackEligibility, StackJob, StackJobKind, StackJobResult, StackJobState,
    StackMergeMethod, StackPlanCheck, StackPlanRequest, StackRewrite, StackRewritePlan,
};

use check::{PlanInput, branches, members, ref_name, stack_number};

use crate::{
    engine::{RostrumCore, state::parse_repo},
    error::RostrumError,
};

impl RostrumCore {
    /// A watched repository as the cached feed has it.
    async fn cached_repo(&self, repo: &RepoId) -> Result<RepoState, RostrumError> {
        self.ensure_hydrated().await?;
        let id = repo.clone();
        self.actor
            .try_call(move |state| {
                state
                    .feed
                    .repos
                    .iter()
                    .find(|watched| watched.id == id)
                    .cloned()
                    .ok_or_else(|| {
                        RostrumError::invalid(format!("{id} is not in the repository list"))
                    })
            })
            .await
    }

    /// A refused stack request, typed. An unconfirmed rewrite is re-planned
    /// so the error names the branches the desktop would rewrite.
    async fn refused(
        &self,
        client: &RemoteClient,
        error: ClientError,
        replan: &PlanInput,
    ) -> RostrumError {
        match error {
            ClientError::Api(api) if api.code == ApiErrorCode::RewriteNotConfirmed => {
                let branches = match client.plan_stack_rewrite(&replan.to_wire()).await {
                    Ok(plan) => StackRewritePlan::from(plan).rewrites,
                    Err(error) => {
                        tracing::warn!(%error, "could not re-plan an unconfirmed stack rewrite");
                        Vec::new()
                    }
                };
                tracing::info!(repo = %replan.repo(), branches = branches.len(), "stack rewrite not confirmed");
                RostrumError::RewriteNotConfirmed {
                    branches,
                    reason: api.message,
                }
            }
            other => other.into(),
        }
    }

    /// The job as Kotlin sees it; the first time it is seen finished, the
    /// repository's feed is refreshed so the change shows.
    async fn settle(&self, status: StackJobStatus) -> StackJob {
        if status.is_finished() {
            let id = status.id;
            let repo = status.repo.clone();
            let first = self
                .actor
                .call(move |state| {
                    let watched = state.feed.repos.iter().any(|known| known.id == repo);
                    state.settled_stack_jobs.insert(id.0) && watched
                })
                .await
                .unwrap_or(false);
            if first {
                tracing::info!(job = %id, repo = %status.repo, "stack job finished; refreshing the repository");
                if let Err(error) = self.refresh_repo(status.repo.to_string()).await {
                    tracing::warn!(repo = %status.repo, %error, "refresh after a stack job failed");
                }
            }
        }
        status.into()
    }

    async fn started(
        &self,
        client: Arc<RemoteClient>,
        result: Result<StackJobStatus, ClientError>,
        replan: &PlanInput,
    ) -> Result<StackJob, RostrumError> {
        match result {
            Ok(status) => {
                tracing::info!(job = %status.id, repo = %status.repo, kind = ?status.kind, "stack job started");
                Ok(self.settle(status).await)
            }
            Err(error) => Err(self.refused(&client, error, replan).await),
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The desktop's dry run: which branches arranging or extending would
    /// rebase and force-push. Show them, then send exactly those names as
    /// `confirm_rewrite`.
    pub async fn plan_stack_rewrite(
        &self,
        request: StackPlanRequest,
    ) -> Result<StackRewritePlan, RostrumError> {
        let input = PlanInput::parse(&request)?;
        let client = self.remote().await?;
        Ok(client.plan_stack_rewrite(&input.to_wire()).await?.into())
    }

    /// The same question answered from the cached feed, with no desktop:
    /// valid (and what it would rewrite as the cache sees it), or why not.
    pub async fn check_stack_plan(
        &self,
        request: StackPlanRequest,
    ) -> Result<StackPlanCheck, RostrumError> {
        let input = PlanInput::parse(&request)?;
        let repo = self.cached_repo(input.repo()).await?;
        Ok(check::check(&repo, &input))
    }

    /// Open pull requests that could be added to the top of stack `stack`,
    /// each with its eligibility, from the cached feed.
    pub async fn stack_candidates(
        &self,
        repo: String,
        stack: u32,
    ) -> Result<Vec<StackCandidate>, RostrumError> {
        let id = parse_repo(&repo)?;
        let stack = stack_number(stack)?;
        let state = self.cached_repo(&id).await?;
        check::candidates(&state, stack)
    }

    /// Make a stack from pull requests whose bases already chain, bottom
    /// first. Nothing is rewritten; a chain that would need it is refused
    /// with `RewriteNotConfirmed` (use `arrange_stack`).
    pub async fn make_stack(
        &self,
        repo: String,
        prs: Vec<u32>,
        trunk: String,
    ) -> Result<StackJob, RostrumError> {
        let request = MakeStackRequest {
            repo: parse_repo(&repo)?,
            prs: members(&prs, 2)?,
            trunk: ref_name(&trunk, "trunk")?,
        };
        let replan = PlanInput::Arrange {
            repo: request.repo.clone(),
            prs: request.prs.clone(),
            trunk: request.trunk.clone(),
        };
        let client = self.remote().await?;
        let result = client.make_stack(&request).await;
        self.started(client, result, &replan).await
    }

    /// Put pull requests into a stack in this order over `trunk`, rebasing
    /// and force-pushing (with lease) exactly `confirm_rewrite`.
    pub async fn arrange_stack(
        &self,
        repo: String,
        prs: Vec<u32>,
        trunk: String,
        confirm_rewrite: Vec<String>,
    ) -> Result<StackJob, RostrumError> {
        let request = ArrangeStackRequest {
            repo: parse_repo(&repo)?,
            prs: members(&prs, 2)?,
            trunk: ref_name(&trunk, "trunk")?,
            confirm_rewrite: branches(&confirm_rewrite)?,
        };
        let replan = PlanInput::Arrange {
            repo: request.repo.clone(),
            prs: request.prs.clone(),
            trunk: request.trunk.clone(),
        };
        let client = self.remote().await?;
        let result = client.arrange_stack(&request).await;
        self.started(client, result, &replan).await
    }

    /// Add pull requests to the top of stack `stack`, bottom first. The
    /// stack's members are never rewritten; additions not already chained
    /// are, and must be named in `confirm_rewrite`.
    pub async fn extend_stack(
        &self,
        repo: String,
        stack: u32,
        prs: Vec<u32>,
        confirm_rewrite: Vec<String>,
    ) -> Result<StackJob, RostrumError> {
        let request = ExtendStackRequest {
            repo: parse_repo(&repo)?,
            stack: stack_number(stack)?,
            prs: members(&prs, 1)?,
            confirm_rewrite: branches(&confirm_rewrite)?,
        };
        let replan = PlanInput::Extend {
            repo: request.repo.clone(),
            stack: request.stack,
            prs: request.prs.clone(),
        };
        let client = self.remote().await?;
        let result = client.extend_stack(&request).await;
        self.started(client, result, &replan).await
    }

    /// GitHub's atomic stack merge: every open member, or none.
    pub async fn merge_stack(
        &self,
        repo: String,
        stack: u32,
        method: StackMergeMethod,
    ) -> Result<StackJob, RostrumError> {
        let request = MergeStackRequest {
            repo: parse_repo(&repo)?,
            stack: stack_number(stack)?,
            method: method.into(),
        };
        let client = self.remote().await?;
        let status = client.merge_stack(&request).await?;
        tracing::info!(job = %status.id, repo = %status.repo, "stack merge started");
        Ok(self.settle(status).await)
    }

    /// Dissolve stack `stack`; its pull requests stay open with their bases.
    pub async fn unstack(&self, repo: String, stack: u32) -> Result<StackJob, RostrumError> {
        let request = UnstackRequest {
            repo: parse_repo(&repo)?,
            stack: stack_number(stack)?,
        };
        let client = self.remote().await?;
        let status = client.unstack(&request).await?;
        tracing::info!(job = %status.id, repo = %status.repo, "unstack started");
        Ok(self.settle(status).await)
    }

    /// Poll a stack job. The desktop keeps only recent jobs: an unknown id is
    /// `RemoteApi` with code `NOT_FOUND`.
    pub async fn stack_job(&self, id: u64) -> Result<StackJob, RostrumError> {
        let client = self.remote().await?;
        let status = client.stack_job(StackJobId(id)).await?;
        Ok(self.settle(status).await)
    }
}
