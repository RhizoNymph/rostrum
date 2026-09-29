//! Bearer-token authentication.
//!
//! Missing header, a scheme other than `Bearer`, a token that is not 32
//! bytes of base64url, a token no device holds, a revoked device's token: all
//! the same 401, so a phone learns only that it must pair again.

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header::AUTHORIZATION, request::Parts},
};
use rostrum_remote::{ApiErrorCode, DeviceId, DeviceToken};

use crate::{
    daemon::Daemon,
    http::{ApiFailure, Peer},
};

/// A request from a paired device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthedDevice {
    pub id: DeviceId,
}

impl FromRequestParts<Daemon> for AuthedDevice {
    type Rejection = ApiFailure;

    async fn from_request_parts(
        parts: &mut Parts,
        daemon: &Daemon,
    ) -> Result<Self, Self::Rejection> {
        let Some(token) = bearer_token(&parts.headers) else {
            return Err(unauthorized(daemon));
        };
        let Peer(ip) = Peer::from_request_parts(parts, daemon).await?;
        match daemon.registry.authenticate(&token, ip).await {
            Some(id) => Ok(Self { id }),
            None => {
                tracing::debug!(client = %ip, "rejected a request with an unknown device token");
                Err(unauthorized(daemon))
            }
        }
    }
}

/// The token in `Authorization: Bearer <token>`, if well-formed.
pub fn bearer_token(headers: &HeaderMap) -> Option<DeviceToken> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    DeviceToken::parse(token.trim()).ok()
}

fn unauthorized(daemon: &Daemon) -> ApiFailure {
    ApiFailure::new(
        ApiErrorCode::Unauthorized,
        format!(
            "this device is not paired with {}; pair it again",
            daemon.machine_name
        ),
    )
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_str(value).expect("header"));
        headers
    }

    #[test]
    fn a_well_formed_bearer_token_is_read() {
        let token = DeviceToken::from_bytes([5; 32]);
        assert_eq!(
            bearer_token(&headers(&format!("Bearer {}", token.expose()))),
            Some(token.clone())
        );
        assert_eq!(
            bearer_token(&headers(&format!("bearer  {} ", token.expose()))),
            Some(token)
        );
    }

    #[test]
    fn anything_else_is_no_token() {
        let token = DeviceToken::from_bytes([5; 32]);
        assert_eq!(bearer_token(&HeaderMap::new()), None);
        assert_eq!(
            bearer_token(&headers(&format!("Basic {}", token.expose()))),
            None
        );
        assert_eq!(bearer_token(&headers("Bearer")), None);
        assert_eq!(bearer_token(&headers("Bearer not-a-token")), None);
        assert_eq!(bearer_token(&headers(token.expose())), None);
    }
}
