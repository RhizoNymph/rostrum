//! The stack routes: a dry run, five operations started as jobs, and polling.
//!
//! Every operation goes clone → lease → snapshot → validate → job; see
//! [`crate::stacks`].

use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use rostrum_config::ConflictHandler;
use rostrum_core::{PrNumber, RepoId, RepoState, plan_extend, plan_stack};
use rostrum_remote::{
    ApiErrorCode, ArrangeStackRequest, ExtendStackRequest, MakeStackRequest, MergeStackRequest,
    StackJobId, StackJobKind, StackJobStatus, StackPlanRequest, StackRewritePlan, UnstackRequest,
};
use rostrum_stack::{ExtendJob, StackJob};

use super::auth::AuthedDevice;
use crate::{
    daemon::Daemon,
    http::{ApiFailure, ApiJson},
    jobs::{CloneKey, Lease},
    stacks::{
        StackRequestError,
        ops::merge_method,
        outcome,
        validate::{self, preview},
    },
};

/// The configured clone a stack operation runs in.
struct Target {
    clone: PathBuf,
    key: CloneKey,
    handler: Option<ConflictHandler>,
}

async fn target(daemon: &Daemon, repo: &RepoId) -> Result<Target, ApiFailure> {
    let config = daemon.rostrum_config.load();
    let clone = config
        .local_path(repo)
        .ok_or_else(|| refusal(StackRequestError::NotConfigured(repo.clone())))?;
    let key = CloneKey::resolve(&clone).await;
    Ok(Target {
        clone,
        key,
        handler: config.conflict_handler.clone(),
    })
}

async fn snapshot(daemon: &Daemon, repo: &RepoId) -> Result<RepoState, ApiFailure> {
    daemon
        .snapshots
        .snapshot(repo)
        .await
        .map_err(|error| ApiFailure::internal("could not read the repository from GitHub", &error))
}

/// The clone, its lease (a busy clone is refused here, first), and GitHub's
/// view of the repository now.
async fn begin(daemon: &Daemon, repo: &RepoId) -> Result<(Target, Lease, RepoState), ApiFailure> {
    let target = target(daemon, repo).await?;
    let lease = daemon.jobs.acquire(target.key.clone()).await?;
    let state = snapshot(daemon, repo).await?;
    Ok((target, lease, state))
}

fn refusal(error: StackRequestError) -> ApiFailure {
    let code = match &error {
        StackRequestError::NotConfigured(_) | StackRequestError::UnknownStack { .. } => {
            ApiErrorCode::NotFound
        }
        StackRequestError::Plan(_) | StackRequestError::Extend(_) => ApiErrorCode::BadRequest,
        StackRequestError::NeedsRewrite { .. } | StackRequestError::Unconfirmed { .. } => {
            ApiErrorCode::RewriteNotConfirmed
        }
    };
    tracing::info!(reason = %error, "refused a stack request");
    ApiFailure::new(code, error.to_string())
}

fn numbers(prs: &[PrNumber]) -> String {
    prs.iter()
        .map(|n| n.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// `POST /api/v1/stacks/plan`: the branches an arrangement or extension
/// would rewrite. Reads only.
pub async fn plan(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<StackPlanRequest>,
) -> Result<Json<StackRewritePlan>, ApiFailure> {
    let repo = match &request {
        StackPlanRequest::Arrange { repo, .. } | StackPlanRequest::Extend { repo, .. } => repo,
    };
    target(&daemon, repo).await?;
    let state = snapshot(&daemon, repo).await?;
    let plan = match &request {
        StackPlanRequest::Arrange { prs, trunk, .. } => {
            let plan = plan_stack(&state, prs, trunk.clone())
                .map_err(|e| refusal(StackRequestError::Plan(e)))?;
            preview(plan.rewrites())
        }
        StackPlanRequest::Extend { stack, prs, .. } => {
            let plan = plan_extend(&state, *stack, prs)
                .map_err(|e| refusal(StackRequestError::Extend(e)))?;
            preview(plan.rewrites())
        }
    };
    Ok(Json(plan))
}

pub async fn make(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<MakeStackRequest>,
) -> Result<Json<StackJobStatus>, ApiFailure> {
    let (target, lease, state) = begin(&daemon, &request.repo).await?;
    let plan = validate::make(&state, &request.prs, request.trunk).map_err(refusal)?;
    tracing::info!(repo = %request.repo, prs = numbers(&request.prs), "starting make stack for a paired device");
    let job = StackJob {
        clone: target.clone,
        plan,
        handler: target.handler,
        scratch_dir: daemon.stack_scratch_dir.clone(),
    };
    let ops = daemon.stack_ops.clone();
    let status = daemon
        .jobs
        .start_stack_job(lease, request.repo, StackJobKind::Make, move |progress| {
            Box::pin(async move { outcome::chain_state(ops.make(job, progress).await) })
        })
        .await?;
    Ok(Json(status))
}

pub async fn arrange(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<ArrangeStackRequest>,
) -> Result<Json<StackJobStatus>, ApiFailure> {
    let (target, lease, state) = begin(&daemon, &request.repo).await?;
    let plan = validate::arrange(
        &state,
        &request.prs,
        request.trunk,
        &request.confirm_rewrite,
    )
    .map_err(refusal)?;
    tracing::info!(
        repo = %request.repo,
        prs = numbers(&request.prs),
        rewrites = numbers(&plan.rewrites().iter().map(|m| m.number).collect::<Vec<_>>()),
        "starting arrange for a paired device"
    );
    let job = StackJob {
        clone: target.clone,
        plan,
        handler: target.handler,
        scratch_dir: daemon.stack_scratch_dir.clone(),
    };
    let ops = daemon.stack_ops.clone();
    let status = daemon
        .jobs
        .start_stack_job(
            lease,
            request.repo,
            StackJobKind::Arrange,
            move |progress| {
                Box::pin(async move { outcome::chain_state(ops.make(job, progress).await) })
            },
        )
        .await?;
    Ok(Json(status))
}

pub async fn extend(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<ExtendStackRequest>,
) -> Result<Json<StackJobStatus>, ApiFailure> {
    let (target, lease, state) = begin(&daemon, &request.repo).await?;
    let plan = validate::extend(
        &state,
        request.stack,
        &request.prs,
        &request.confirm_rewrite,
    )
    .map_err(refusal)?;
    tracing::info!(
        repo = %request.repo,
        stack = request.stack.get(),
        prs = numbers(&request.prs),
        rewrites = numbers(&plan.rewrites().iter().map(|m| m.number).collect::<Vec<_>>()),
        "starting add to stack for a paired device"
    );
    let job = ExtendJob {
        clone: target.clone,
        plan,
        handler: target.handler,
        scratch_dir: daemon.stack_scratch_dir.clone(),
    };
    let ops = daemon.stack_ops.clone();
    let status = daemon
        .jobs
        .start_stack_job(lease, request.repo, StackJobKind::Extend, move |progress| {
            Box::pin(async move { outcome::chain_state(ops.extend(job, progress).await) })
        })
        .await?;
    Ok(Json(status))
}

pub async fn merge(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<MergeStackRequest>,
) -> Result<Json<StackJobStatus>, ApiFailure> {
    let (target, lease, state) = begin(&daemon, &request.repo).await?;
    validate::existing_stack(&state, request.stack).map_err(refusal)?;
    tracing::info!(
        repo = %request.repo,
        stack = request.stack.get(),
        method = ?request.method,
        "starting merge stack for a paired device"
    );
    let ops = daemon.stack_ops.clone();
    let (repo, stack, method) = (
        request.repo.clone(),
        request.stack,
        merge_method(request.method),
    );
    let status = daemon
        .jobs
        .start_stack_job(lease, request.repo, StackJobKind::Merge, move |progress| {
            Box::pin(async move {
                outcome::merge_state(
                    stack,
                    ops.merge(target.clone, repo, stack, method, progress).await,
                )
            })
        })
        .await?;
    Ok(Json(status))
}

pub async fn unstack(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<UnstackRequest>,
) -> Result<Json<StackJobStatus>, ApiFailure> {
    let (target, lease, state) = begin(&daemon, &request.repo).await?;
    validate::existing_stack(&state, request.stack).map_err(refusal)?;
    tracing::info!(repo = %request.repo, stack = request.stack.get(), "starting unstack for a paired device");
    let ops = daemon.stack_ops.clone();
    let (repo, stack) = (request.repo.clone(), request.stack);
    let status = daemon
        .jobs
        .start_stack_job(
            lease,
            request.repo,
            StackJobKind::Unstack,
            move |progress| {
                Box::pin(async move {
                    outcome::unstack_state(
                        stack,
                        ops.unstack(target.clone, repo, stack, progress).await,
                    )
                })
            },
        )
        .await?;
    Ok(Json(status))
}

/// `GET /api/v1/stacks/jobs/{id}`.
pub async fn job(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    Path(id): Path<String>,
) -> Result<Json<StackJobStatus>, ApiFailure> {
    let id = id
        .parse::<u64>()
        .map(StackJobId)
        .map_err(|_| ApiFailure::new(ApiErrorCode::BadRequest, "that is not a stack job id"))?;
    daemon.jobs.stack_job(id).await?.map(Json).ok_or_else(|| {
        ApiFailure::new(
            ApiErrorCode::NotFound,
            format!("no stack job {id} on this computer; it may be too old to remember"),
        )
    })
}
