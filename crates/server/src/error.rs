//! The JSON error every API failure returns.

use std::fmt::Display;

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::RETRY_AFTER},
    response::{IntoResponse, Response},
};
use jiff::SignedDuration;
use serde::Serialize;

/// An API failure: a status, a stable machine code, and a human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    retry_after_secs: Option<i64>,
}

/// The response body of an [`ApiError`].
#[derive(Debug, Serialize)]
struct Body<'a> {
    error: &'a str,
    message: &'a str,
}

impl ApiError {
    /// A failure with `status`, `code`, and `message`.
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after_secs: None,
        }
    }

    /// 400: the request is malformed or breaks a rule.
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    /// 401: no live session.
    #[must_use]
    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "sign in required",
        )
    }

    /// 403: the caller may not do this.
    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    /// 404: no such resource.
    #[must_use]
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "not found")
    }

    /// 409: the request conflicts with the current state.
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    /// 429: try again after `retry_after`.
    pub fn too_many(
        code: &'static str,
        message: impl Into<String>,
        retry_after: SignedDuration,
    ) -> Self {
        let mut error = Self::new(StatusCode::TOO_MANY_REQUESTS, code, message);
        // Round up, so a client that waits exactly this long is let in.
        let secs = retry_after.as_secs() + i64::from(retry_after.subsec_nanos() > 0);
        error.retry_after_secs = Some(secs.max(1));
        error
    }

    /// 500: logs `error` and hides it from the client.
    pub fn internal(error: impl Display) -> Self {
        tracing::error!(%error, "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "an internal error occurred",
        )
    }

    /// The HTTP status.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// The machine code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(Body {
                error: self.code,
                message: &self.message,
            }),
        )
            .into_response();
        if let Some(secs) = self.retry_after_secs {
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from(secs));
        }
        response
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl From<saffron_db::sessions::SessionError> for ApiError {
    fn from(error: saffron_db::sessions::SessionError) -> Self {
        Self::internal(error)
    }
}

impl From<saffron_db::login_throttle::ThrottleError> for ApiError {
    fn from(error: saffron_db::login_throttle::ThrottleError) -> Self {
        Self::internal(error)
    }
}

/// The result of an API handler.
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn retry_after(error: ApiError) -> Option<String> {
        error
            .into_response()
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    #[test]
    fn retry_after_rounds_a_partial_second_up() {
        let error = ApiError::too_many("slow", "slow", SignedDuration::from_millis(1500));

        assert_eq!(retry_after(error).as_deref(), Some("2"));
    }

    #[test]
    fn retry_after_is_at_least_one_second() {
        let error = ApiError::too_many("slow", "slow", SignedDuration::ZERO);

        assert_eq!(retry_after(error).as_deref(), Some("1"));
    }

    #[test]
    fn other_errors_have_no_retry_after() {
        assert_eq!(retry_after(ApiError::not_found()), None);
    }
}
