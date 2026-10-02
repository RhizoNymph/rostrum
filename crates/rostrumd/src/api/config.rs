//! `PUT /api/v1/config`: a phone sending its settings to the desktop.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rostrum_remote::{ApiErrorCode, ConfigConflict, ConfigPush};

use super::auth::AuthedDevice;
use crate::{
    config_push::{PushResult, validate},
    daemon::Daemon,
    http::{ApiFailure, ApiJson},
};

pub async fn push(
    State(daemon): State<Daemon>,
    device: AuthedDevice,
    ApiJson(push): ApiJson<ConfigPush>,
) -> Result<Response, ApiFailure> {
    validate(&push.config).map_err(|error| {
        tracing::info!(device = %device.id, reason = %error, "refused a settings push");
        ApiFailure::new(ApiErrorCode::BadRequest, error.to_string())
    })?;
    let conditional = push.base.is_some();
    match daemon.config_writer.push(push).await {
        Ok(PushResult::Applied { config, changed }) => {
            tracing::info!(
                device = %device.id,
                changed = ?changed,
                conditional,
                revision = %config.revision,
                "applied settings pushed by a paired device"
            );
            Ok(Json(config).into_response())
        }
        Ok(PushResult::Changed(current)) => {
            tracing::info!(
                device = %device.id,
                revision = %current.revision,
                "refused a settings push: the desktop's settings changed since its base"
            );
            let body = ConfigConflict {
                code: ApiErrorCode::ConfigChanged,
                message:
                    "the desktop's settings changed since this push was previewed; preview again"
                        .into(),
                current,
            };
            Ok((StatusCode::CONFLICT, Json(body)).into_response())
        }
        Err(error) => Err(ApiFailure::internal("could not save the settings", &error)),
    }
}
