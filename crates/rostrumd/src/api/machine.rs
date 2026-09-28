//! `machine`, `github-token`, `handoffs` and `DELETE device`.

use axum::{Json, extract::State};
use rostrum_remote::{ApiErrorCode, GitHubHandover, HandoffSession, MachineInfo};

use super::auth::AuthedDevice;
use crate::{daemon::Daemon, http::ApiFailure, tmux};

pub async fn machine(State(daemon): State<Daemon>, _device: AuthedDevice) -> Json<MachineInfo> {
    Json(daemon.machine_info())
}

pub async fn github_token(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
) -> Result<Json<GitHubHandover>, ApiFailure> {
    daemon.github.handover().await.map(Json).ok_or_else(|| {
        ApiFailure::new(
            ApiErrorCode::NotFound,
            format!(
                "{} has no GitHub token: run `gh auth login` there, or set GITHUB_TOKEN for rostrumd",
                daemon.machine_name
            ),
        )
    })
}

pub async fn handoffs(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
) -> Result<Json<Vec<HandoffSession>>, ApiFailure> {
    let sessions = daemon
        .tmux
        .list()
        .await
        .map_err(|error| ApiFailure::internal("could not list tmux sessions", &error))?;
    let records = daemon.jobs.handoff_records().await?;
    Ok(Json(tmux::handoff_sessions(sessions, &records)))
}

/// Forget the calling device. Its token stops working immediately.
pub async fn forget(
    State(daemon): State<Daemon>,
    device: AuthedDevice,
) -> Result<Json<()>, ApiFailure> {
    daemon
        .registry
        .revoke(device.id)
        .await
        .map_err(|error| ApiFailure::internal("could not forget the device", &error))?;
    Ok(Json(()))
}
