//! `local/push/plan` and `local/push`: Push and Push all from a phone.
//!
//! Every value is validated before anything runs — head branch names, and
//! each confirmation's oid — so a request that cannot be valid is a 400 and
//! touches nothing. Then each pull request is planned or pushed in turn,
//! holding its clone's lease, with its own verdict: a busy clone or a
//! refused push is that entry's answer, never the batch's, so the phone
//! always learns what happened to every branch.
//!
//! The push itself is [`rostrum_local::run_push`], the desktop's own: it
//! re-reads the branch from the refs on disk and rewrites a diverged branch
//! only when the request confirmed that pull request with exactly the remote
//! oid the lease would name.

use std::collections::HashMap;

use axum::{Json, extract::State};
use rostrum_git::Oid;
use rostrum_local::{ConfirmedRewrite, PushTarget, Refresh, plan_push, run_push};
use rostrum_remote::{
    ApiErrorCode, PrKey, PushEntry, PushOutcomeView, PushPlanEntry, PushPlanRequest,
    PushPlanResponse, PushPlanView, PushRequest, PushResponse, PushTargetRef,
};

use super::auth::AuthedDevice;
use crate::{
    convert,
    daemon::Daemon,
    http::{ApiFailure, ApiJson},
    jobs::CloneKey,
};

/// A requested pull request, validated and resolved against the config.
struct Resolved {
    pr: PushTargetRef,
    /// `None` when no clone of the repository is configured.
    target: Option<(CloneKey, PushTarget)>,
}

async fn resolve(
    config: &::rostrum_config::Config,
    prs: Vec<PushTargetRef>,
) -> Result<Vec<Resolved>, ApiFailure> {
    let mut resolved = Vec::with_capacity(prs.len());
    for pr in prs {
        let branch = convert::branch(&pr.head_ref)
            .map_err(|reason| ApiFailure::new(ApiErrorCode::BadRequest, reason))?;
        let target = match config.local_path(&pr.key.repo) {
            None => None,
            Some(clone) => Some((
                CloneKey::resolve(&clone).await,
                PushTarget {
                    clone,
                    branch,
                    head_repo: pr.head_repo.clone(),
                },
            )),
        };
        resolved.push(Resolved { pr, target });
    }
    Ok(resolved)
}

pub async fn plan(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<PushPlanRequest>,
) -> Result<Json<PushPlanResponse>, ApiFailure> {
    let config = daemon.rostrum_config.load();
    let mut entries = Vec::with_capacity(request.prs.len());
    for Resolved { pr, target } in resolve(&config, request.prs).await? {
        let plan = match target {
            None => PushPlanView::NotConfigured,
            Some((key, target)) => match daemon
                .jobs
                .exclusive(key, async move { plan_push(&target, Refresh::Fetch).await })
                .await
            {
                Ok(plan) => convert::push_plan_view(plan),
                Err(busy) => PushPlanView::Failed {
                    reason: busy.to_string(),
                },
            },
        };
        entries.push(PushPlanEntry {
            key: pr.key,
            head_ref: pr.head_ref,
            plan,
        });
    }
    Ok(Json(PushPlanResponse { entries }))
}

/// The request's confirmations by pull request, each oid validated. Two
/// different oids for one pull request cannot both be meant: 400.
fn confirmations(request: &PushRequest) -> Result<HashMap<PrKey, Oid>, ApiFailure> {
    let mut confirmed: HashMap<PrKey, Oid> = HashMap::new();
    for confirmation in &request.confirm_rewrite {
        let oid = Oid::parse(confirmation.expected.clone())
            .map_err(|error| ApiFailure::new(ApiErrorCode::BadRequest, error.to_string()))?;
        if let Some(previous) = confirmed.insert(confirmation.key.clone(), oid.clone())
            && previous != oid
        {
            return Err(ApiFailure::new(
                ApiErrorCode::BadRequest,
                format!(
                    "{}{} is confirmed against two different oids",
                    confirmation.key.repo, confirmation.key.number
                ),
            ));
        }
    }
    Ok(confirmed)
}

pub async fn push(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<PushRequest>,
) -> Result<Json<PushResponse>, ApiFailure> {
    let confirmed = confirmations(&request)?;
    let config = daemon.rostrum_config.load();
    let mut entries = Vec::with_capacity(request.prs.len());
    for Resolved { pr, target } in resolve(&config, request.prs).await? {
        let outcome = match target {
            None => PushOutcomeView::NotConfigured,
            Some((key, target)) => {
                // The confirmation names the branch it is for, so it can only
                // ever authorize this pull request's own branch.
                let confirmation = confirmed.get(&pr.key).map(|expected| ConfirmedRewrite {
                    branch: target.branch.clone(),
                    expected: expected.clone(),
                });
                tracing::info!(
                    repo = %pr.key.repo,
                    number = pr.key.number.0,
                    confirmed = confirmation.as_ref().map(|c| c.expected.short().to_string()),
                    "pushing a branch for a paired device"
                );
                match daemon
                    .jobs
                    .exclusive(key, async move {
                        run_push(&target, confirmation.as_ref()).await
                    })
                    .await
                {
                    Ok(result) => convert::push_outcome_view(result),
                    Err(busy) => PushOutcomeView::Failed {
                        reason: busy.to_string(),
                    },
                }
            }
        };
        tracing::info!(
            repo = %pr.key.repo,
            number = pr.key.number.0,
            outcome = %outcome.detail(),
            "push finished"
        );
        entries.push(PushEntry {
            key: pr.key,
            head_ref: pr.head_ref,
            outcome,
        });
    }
    Ok(Json(PushResponse { entries }))
}
