//! One structured line per request.
//!
//! Only the method, the path, the status, the peer and the duration. Never a
//! header (the `Authorization` header is a device token) and never a query
//! string or a body (a pairing code travels in a body).

use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};

use super::peer_ip;

pub async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let client = peer_ip(request.extensions());
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let client = client.map(|ip| ip.to_string()).unwrap_or_default();
    if status >= 500 {
        tracing::warn!(%method, path, status, client, elapsed_ms, "request failed");
    } else {
        tracing::debug!(%method, path, status, client, elapsed_ms, "request");
    }
    response
}
