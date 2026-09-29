//! Who may generate pairing codes, list paired devices, or revoke one.
//!
//! Three checks, all of which must pass:
//!
//! 1. The peer is this computer or the tailnet ([`ClientClass`]) — the
//!    owner's rule.
//! 2. The request is addressed (`Host`) to an IP literal or one of this
//!    computer's own names ([`TrustedNames`]) — DNS-rebinding defence.
//! 3. For anything but `GET`/`HEAD`, an `Origin` header, when present, names
//!    the same origin as `Host` — cross-site request defence.

use std::convert::Infallible;

use axum::{
    extract::FromRequestParts,
    http::{Method, header, request::Parts},
};
use rostrum_remote::ApiErrorCode;

use crate::{
    daemon::Daemon,
    http::{ApiFailure, peer_ip},
    net::{
        ClientClass,
        request_host::{RequestAuthority, origin_matches},
    },
};

/// Why a request may not administer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("pairing codes can only be generated from this computer or over the tailnet")]
    NotLocal,
    #[error(
        "this page was opened under a name rostrumd does not answer to for pairing; open it by IP address, localhost, or the tailnet name"
    )]
    UntrustedHost,
    #[error("the request came from another site")]
    CrossOrigin,
    #[error("the peer address is unknown")]
    NoPeer,
}

/// Decide whether the request in `parts` may administer.
pub fn admit(parts: &Parts, daemon: &Daemon) -> Result<ClientClass, Refusal> {
    let ip = peer_ip(&parts.extensions).ok_or(Refusal::NoPeer)?;
    let class = ClientClass::of(ip);
    if !class.may_administer() {
        return Err(Refusal::NotLocal);
    }
    let authority = parts
        .headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(RequestAuthority::parse)
        .ok_or(Refusal::UntrustedHost)?;
    if !daemon.trusted_names().admits(&authority) {
        return Err(Refusal::UntrustedHost);
    }
    let reads = parts.method == Method::GET || parts.method == Method::HEAD;
    if !reads && let Some(origin) = parts.headers.get(header::ORIGIN) {
        let same = origin
            .to_str()
            .is_ok_and(|origin| origin_matches(origin, &authority));
        if !same {
            return Err(Refusal::CrossOrigin);
        }
    }
    Ok(class)
}

/// Proof that the request may administer; otherwise a 403 `forbidden`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Admin(pub ClientClass);

impl FromRequestParts<Daemon> for Admin {
    type Rejection = ApiFailure;

    async fn from_request_parts(
        parts: &mut Parts,
        daemon: &Daemon,
    ) -> Result<Self, Self::Rejection> {
        admit(parts, daemon).map(Self).map_err(|refusal| {
            tracing::info!(
                client = ?peer_ip(&parts.extensions),
                path = parts.uri.path(),
                reason = %refusal,
                "refused a privileged page request"
            );
            ApiFailure::new(ApiErrorCode::Forbidden, refusal.to_string())
        })
    }
}

/// Which variant of the page to render. Never rejects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Admin,
    Visitor(Refusal),
}

impl FromRequestParts<Daemon> for Access {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        daemon: &Daemon,
    ) -> Result<Self, Self::Rejection> {
        Ok(match admit(parts, daemon) {
            Ok(_) => Self::Admin,
            Err(refusal) => Self::Visitor(refusal),
        })
    }
}
