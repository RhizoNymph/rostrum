//! `local/status`, `local/job` and `local/abort`: one pull request's clone.
//!
//! Each reads rostrum's config afresh to find the clone, validates the branch
//! names before they reach git, and runs the git work under the clone's lease
//! (see [`crate::jobs`]).

use axum::{Json, extract::State};
use rostrum_git::{BranchName, GitError};
use rostrum_handoff::session_name;
use rostrum_local::{LocalJob, LocalState, abort_in_progress, local_state};
use rostrum_remote::{
    AbortRequest, ApiErrorCode, JobOutcome, JobRequest, LocalStatus, LocalStatusRequest,
};

use super::auth::AuthedDevice;
use crate::{
    convert,
    daemon::Daemon,
    http::{ApiFailure, ApiJson},
    jobs::CloneKey,
};

fn branch(raw: &str) -> Result<BranchName, ApiFailure> {
    convert::branch(raw).map_err(|reason| ApiFailure::new(ApiErrorCode::BadRequest, reason))
}

pub async fn status(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<LocalStatusRequest>,
) -> Result<Json<LocalStatus>, ApiFailure> {
    let head = branch(&request.head_ref)?;
    let config = daemon.rostrum_config.load();
    let Some(clone) = config.local_path(&request.key.repo) else {
        return Ok(Json(LocalStatus::NotConfigured));
    };
    // The session a conflict here would have been handed to — asked about
    // only when a handler could have made one.
    let session = config
        .conflict_handler
        .as_ref()
        .map(|_| session_name(&request.key.repo, request.key.number));
    let autostash = convert::autostash(config.autostash);
    let key = CloneKey::resolve(&clone).await;
    let state = daemon
        .jobs
        .exclusive(key, async move {
            local_state(&clone, head, autostash, session).await
        })
        .await?
        .map_err(|error| ApiFailure::internal("could not read the clone", &error))?;
    Ok(Json(convert::local_status(state)))
}

pub async fn job(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<JobRequest>,
) -> Result<Json<JobOutcome>, ApiFailure> {
    let head = branch(&request.pr.head_ref)?;
    let base = branch(&request.pr.base_ref)?;
    let config = daemon.rostrum_config.load();
    let Some(clone) = config.local_path(&request.pr.key.repo) else {
        return Ok(Json(JobOutcome::NotConfigured));
    };
    let key = CloneKey::resolve(&clone).await;
    tracing::info!(
        repo = %request.pr.key.repo,
        number = request.pr.key.number.0,
        op = ?request.op,
        autostash = request.autostash,
        "running a local job for a paired device"
    );
    let job = LocalJob {
        clone,
        branch: head,
        base,
        op: convert::local_op(request.op),
        autostash: convert::autostash(request.autostash),
        handler: config.conflict_handler.clone(),
        pr: convert::pr_meta(&request.pr),
    };
    let result = daemon
        .jobs
        .run_job(key, job, request.pr.key.clone())
        .await?;
    tracing::info!(
        repo = %request.pr.key.repo,
        number = request.pr.key.number.0,
        outcome = %result.detail(),
        "local job finished"
    );
    Ok(Json(convert::job_outcome(result)))
}

/// Why an abort did not happen, when that is the caller's to fix.
enum AbortRefusal {
    NotCheckedOut,
    /// Nothing in progress, or something rostrum does not abort (a bisect, a
    /// `git am`).
    NothingToAbort(Option<rostrum_git::InProgress>),
}

pub async fn abort(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<AbortRequest>,
) -> Result<Json<()>, ApiFailure> {
    let head = branch(&request.head_ref)?;
    let config = daemon.rostrum_config.load();
    let Some(clone) = config.local_path(&request.key.repo) else {
        return Err(ApiFailure::new(
            ApiErrorCode::NotFound,
            format!(
                "no clone of {} is configured on {}",
                request.key.repo, daemon.machine_name
            ),
        ));
    };
    let autostash = convert::autostash(config.autostash);
    let key = CloneKey::resolve(&clone).await;
    let outcome = daemon
        .jobs
        .exclusive(key, async move {
            let worktree = match local_state(&clone, head, autostash, None).await? {
                LocalState::NotCheckedOut => return Ok(Err(AbortRefusal::NotCheckedOut)),
                LocalState::CheckedOut(branch) => branch.worktree,
            };
            match abort_in_progress(&worktree).await {
                Ok(()) => Ok(Ok(worktree)),
                Err(GitError::NothingToDescribe { in_progress }) => {
                    Ok(Err(AbortRefusal::NothingToAbort(in_progress)))
                }
                Err(error) => Err(error),
            }
        })
        .await?
        .map_err(|error: GitError| ApiFailure::internal("could not abort", &error))?;
    match outcome {
        Ok(worktree) => {
            tracing::info!(
                repo = %request.key.repo,
                number = request.key.number.0,
                worktree = %worktree.display(),
                "aborted an operation for a paired device"
            );
            Ok(Json(()))
        }
        Err(AbortRefusal::NotCheckedOut) => Err(ApiFailure::new(
            ApiErrorCode::NotFound,
            format!("`{}` is not checked out in any worktree", request.head_ref),
        )),
        Err(AbortRefusal::NothingToAbort(None)) => Err(ApiFailure::new(
            ApiErrorCode::BadRequest,
            format!(
                "nothing is in progress where `{}` is checked out",
                request.head_ref
            ),
        )),
        Err(AbortRefusal::NothingToAbort(Some(other))) => Err(ApiFailure::new(
            ApiErrorCode::BadRequest,
            format!(
                "{}; only rebases and merges can be aborted from the phone",
                other.describe()
            ),
        )),
    }
}
