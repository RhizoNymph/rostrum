//! The page server: plain HTTP, so any browser on the LAN or tailnet opens it
//! without a certificate warning.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /` | anyone; the pairing and device panels only for [`gate::Admin`] |
//! | `GET /rostrum.apk` | anyone |
//! | `POST /pairing-codes` | [`gate::Admin`] |
//! | `GET /devices` | [`gate::Admin`] |
//! | `POST /devices/{id}/revoke` | [`gate::Admin`] |

pub mod apk;
mod codes;
mod devices;
pub mod gate;
pub mod page;
pub mod qr;

use axum::{
    Router,
    extract::Request,
    http::{HeaderName, HeaderValue, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use rostrum_remote::ApiErrorCode;

pub use codes::CodeOffer;

use crate::{
    daemon::Daemon,
    http::{ApiFailure, log_request},
};

pub fn router(daemon: Daemon) -> Router {
    Router::new()
        .route("/", get(page::index))
        .route("/rostrum.apk", get(apk::download))
        .route("/pairing-codes", post(codes::issue))
        .route("/devices", get(devices::list))
        .route("/devices/{id}/revoke", post(devices::revoke))
        .fallback(not_found)
        .layer(middleware::from_fn(hardening_headers))
        .layer(middleware::from_fn(log_request))
        .with_state(daemon)
}

async fn not_found() -> ApiFailure {
    ApiFailure::new(ApiErrorCode::NotFound, "no such page")
}

/// The page makes no external requests, so the policy can say so: nothing
/// loads from anywhere but inline, and fetches go only to this server.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; \
script-src 'unsafe-inline'; img-src data:; connect-src 'self'; base-uri 'none'; \
form-action 'none'; frame-ancestors 'none'";

async fn hardening_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let set = |headers: &mut axum::http::HeaderMap, name: HeaderName, value: &'static str| {
        headers
            .entry(name)
            .or_insert(HeaderValue::from_static(value));
    };
    set(headers, header::CACHE_CONTROL, "no-store");
    set(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(headers, header::REFERRER_POLICY, "no-referrer");
    set(headers, header::X_FRAME_OPTIONS, "DENY");
    set(
        headers,
        header::CONTENT_SECURITY_POLICY,
        CONTENT_SECURITY_POLICY,
    );
    response
}

#[cfg(test)]
mod tests;
