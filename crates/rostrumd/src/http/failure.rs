//! Every non-2xx answer is an [`ApiError`] with its code's status.

use axum::{
    Json,
    extract::{FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rostrum_remote::{ApiError, ApiErrorCode};
use serde::de::DeserializeOwned;

use crate::jobs::{Busy, JobsError};

/// An [`ApiError`] on its way out as a response.
#[derive(Debug)]
pub struct ApiFailure(pub ApiError);

impl ApiFailure {
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self(ApiError::new(code, message))
    }

    pub fn code(&self) -> ApiErrorCode {
        self.0.code
    }

    /// A server-side failure: logged in full, answered with the same text.
    pub fn internal(context: &str, error: &dyn std::error::Error) -> Self {
        tracing::error!(%error, context, "request failed");
        Self::new(ApiErrorCode::Internal, format!("{context}: {error}"))
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self.0)).into_response()
    }
}

impl From<JobsError> for ApiFailure {
    fn from(error: JobsError) -> Self {
        match error {
            JobsError::Busy(busy) => Self::from(busy),
            JobsError::Crashed(_) | JobsError::Stopped => Self::internal("local job", &error),
        }
    }
}

impl From<Busy> for ApiFailure {
    fn from(busy: Busy) -> Self {
        Self::new(ApiErrorCode::Busy, busy.to_string())
    }
}

/// `Json<T>`, but a body that does not parse is a 400 [`ApiError`] rather
/// than axum's plain-text rejection.
#[derive(Debug)]
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiFailure;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiFailure::new(
                ApiErrorCode::BadRequest,
                rejection.body_text(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_failure_answers_with_its_codes_status_and_body() {
        let response = ApiFailure::new(ApiErrorCode::PairingCodeExpired, "late").into_response();
        assert_eq!(response.status(), StatusCode::GONE);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let error: ApiError = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            error,
            ApiError::new(ApiErrorCode::PairingCodeExpired, "late")
        );
    }

    #[test]
    fn busy_is_409() {
        let failure = ApiFailure::from(JobsError::Busy(Busy::Job));
        assert_eq!(failure.code(), ApiErrorCode::Busy);
        assert_eq!(failure.code().http_status(), 409);
    }
}
