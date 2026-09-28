//! `hello` and `pair`: the two routes a phone uses before it has a token.

use axum::{Json, extract::State};
use rostrum_remote::{API_VERSION, ApiErrorCode, Hello, PairRequest, PairResponse};

use crate::{
    daemon::Daemon,
    http::{ApiFailure, ApiJson, Peer},
    registry::{PairError, RedeemError},
};

pub async fn hello(State(daemon): State<Daemon>) -> Json<Hello> {
    Json(Hello {
        machine: daemon.machine_name.clone(),
        api_version: API_VERSION,
    })
}

/// Exchange a pairing code for a device token (and the desktop's GitHub
/// token, when it has one).
pub async fn pair(
    State(daemon): State<Daemon>,
    Peer(ip): Peer,
    ApiJson(request): ApiJson<PairRequest>,
) -> Result<Json<PairResponse>, ApiFailure> {
    let paired = match daemon
        .registry
        .pair(
            request.code,
            request.device_name,
            request.replaces.as_ref(),
            ip,
        )
        .await
    {
        Ok(paired) => paired,
        Err(PairError::Code(refusal)) => {
            tracing::warn!(client = %ip, reason = %refusal, "rejected a pairing attempt");
            return Err(ApiFailure::new(redeem_code(refusal), refusal.to_string()));
        }
        Err(PairError::Registry(error)) => {
            return Err(ApiFailure::internal("could not record the device", &error));
        }
    };
    let github = daemon.github.handover().await;
    Ok(Json(PairResponse {
        device: paired.device,
        token: paired.token,
        machine: daemon.machine_info(),
        github,
    }))
}

fn redeem_code(refusal: RedeemError) -> ApiErrorCode {
    match refusal {
        RedeemError::RateLimited => ApiErrorCode::RateLimited,
        RedeemError::Expired => ApiErrorCode::PairingCodeExpired,
        RedeemError::Invalid | RedeemError::Burned => ApiErrorCode::PairingCodeInvalid,
    }
}
