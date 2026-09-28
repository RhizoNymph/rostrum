//! The address a request came from.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{Extensions, request::Parts},
};
use rostrum_remote::ApiErrorCode;

use super::ApiFailure;

/// The peer's IP, canonical (an IPv4-mapped IPv6 address is its IPv4).
pub fn peer_ip(extensions: &Extensions) -> Option<IpAddr> {
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_canonical())
}

/// Extracts [`peer_ip`]. Both servers are built with connect info, so its
/// absence is a server bug; the request is refused rather than guessed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer(pub IpAddr);

impl<S: Send + Sync> FromRequestParts<S> for Peer {
    type Rejection = ApiFailure;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        peer_ip(&parts.extensions).map(Self).ok_or_else(|| {
            tracing::error!("a request arrived without a peer address");
            ApiFailure::new(ApiErrorCode::Internal, "the peer address is unknown")
        })
    }
}
