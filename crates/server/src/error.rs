//! Failures of the JSON API and their mapping to HTTP responses.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use schisaciaccole_core::api::{ApiError, error_codes};
use thiserror::Error;
use tracing::error;

use crate::crypto::CryptoError;
use crate::tokens::TokenError;

/// Failure of a JSON API handler.
///
/// Turned into a response whose body is an [`ApiError`]. Internal failures
/// are logged and answered with a generic message, so their details never
/// reach the client.
#[derive(Debug, Error)]
pub enum ApiFailure {
    /// Malformed request or invalid parameters: 400 `invalid_request`.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Unknown, expired or reused one-time code, or wrong PKCE verifier:
    /// 400 `invalid_code`.
    #[error("invalid code")]
    InvalidCode,

    /// Missing or invalid credentials: 401 `unauthorized`.
    #[error("unauthorized")]
    Unauthorized,

    /// Database failure: 500 `internal`.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Access token signing failure: 500 `internal`.
    #[error(transparent)]
    Token(#[from] TokenError),

    /// Random generator or encryption failure: 500 `internal`.
    #[error(transparent)]
    Crypto(#[from] CryptoError),

    /// Any other unexpected condition: 500 `internal`.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiFailure {
    /// Status code, error code and client-facing message of this failure.
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::InvalidRequest(message) => (
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_REQUEST,
                message.clone(),
            ),
            Self::InvalidCode => (
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_CODE,
                "invalid or expired code".to_string(),
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_codes::UNAUTHORIZED,
                "missing, invalid or expired credentials".to_string(),
            ),
            Self::Database(_) | Self::Token(_) | Self::Crypto(_) | Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_codes::INTERNAL,
                "internal server error".to_string(),
            ),
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if status.is_server_error() {
            error!(error = %self, "request failed");
        }
        let body = ApiError {
            code: code.to_string(),
            message,
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;

    use super::*;

    async fn body_of(failure: ApiFailure) -> (StatusCode, ApiError) {
        let response = failure.into_response();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn invalid_request_is_400_with_message() {
        let (status, body) = body_of(ApiFailure::InvalidRequest("bad port".to_string())).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.code, "invalid_request");
        assert_eq!(body.message, "bad port");
    }

    #[tokio::test]
    async fn invalid_code_is_400() {
        let (status, body) = body_of(ApiFailure::InvalidCode).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.code, "invalid_code");
    }

    #[tokio::test]
    async fn unauthorized_is_401() {
        let (status, body) = body_of(ApiFailure::Unauthorized).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.code, "unauthorized");
    }

    #[tokio::test]
    async fn database_error_is_500_without_details() {
        let (status, body) = body_of(ApiFailure::Database(sqlx::Error::RowNotFound)).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.code, "internal");
        assert_eq!(body.message, "internal server error");
    }
}
