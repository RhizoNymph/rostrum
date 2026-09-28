//! The HTTPS API: every route in [`rostrum_remote::routes`].
//!
//! `hello` is open, `pair` is authenticated by its pairing code, and every
//! other route by the `Authorization: Bearer <device token>` header
//! ([`auth::AuthedDevice`]). Bodies are JSON both ways; a route with nothing
//! to say answers `null`; a failure is an [`rostrum_remote::ApiError`] with
//! its code's status.

pub mod auth;
mod local;
mod machine;
mod pairing;
mod sync;

use axum::{
    Router, middleware,
    routing::{delete, get, post},
};
use rostrum_remote::{ApiErrorCode, routes};

use crate::{
    daemon::Daemon,
    http::{ApiFailure, log_request},
};

pub fn router(daemon: Daemon) -> Router {
    Router::new()
        .route(routes::HELLO, get(pairing::hello))
        .route(routes::PAIR, post(pairing::pair))
        .route(routes::MACHINE, get(machine::machine))
        .route(routes::GITHUB_TOKEN, get(machine::github_token))
        .route(routes::LOCAL_STATUS, post(local::status))
        .route(routes::LOCAL_JOB, post(local::job))
        .route(routes::LOCAL_ABORT, post(local::abort))
        .route(routes::SYNC_ALL, post(sync::start).get(sync::latest))
        .route(routes::HANDOFFS, get(machine::handoffs))
        .route(routes::DEVICE, delete(machine::forget))
        .fallback(not_found)
        .layer(middleware::from_fn(log_request))
        .with_state(daemon)
}

async fn not_found() -> ApiFailure {
    ApiFailure::new(ApiErrorCode::NotFound, "no such route")
}

#[cfg(test)]
mod tests;
