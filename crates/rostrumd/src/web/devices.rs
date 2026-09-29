//! `GET /devices` and `POST /devices/{id}/revoke`.

use axum::{
    Json,
    extract::{Path, State},
};
use rostrum_remote::{ApiErrorCode, DeviceId};

use super::gate::Admin;
use crate::{daemon::Daemon, http::ApiFailure, registry::DeviceView};

pub async fn list(
    State(daemon): State<Daemon>,
    _admin: Admin,
) -> Result<Json<Vec<DeviceView>>, ApiFailure> {
    daemon
        .registry
        .devices()
        .await
        .map(Json)
        .map_err(|error| ApiFailure::internal("could not list devices", &error))
}

pub async fn revoke(
    State(daemon): State<Daemon>,
    _admin: Admin,
    Path(id): Path<String>,
) -> Result<Json<()>, ApiFailure> {
    let id = DeviceId::parse(&id)
        .map_err(|_| ApiFailure::new(ApiErrorCode::BadRequest, "that is not a device id"))?;
    match daemon.registry.revoke(id).await {
        Ok(true) => Ok(Json(())),
        Ok(false) => Err(ApiFailure::new(
            ApiErrorCode::NotFound,
            "no such device; it may already have been revoked",
        )),
        Err(error) => Err(ApiFailure::internal("could not revoke the device", &error)),
    }
}
