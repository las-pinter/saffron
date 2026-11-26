//! Body and path extractors whose rejections are JSON [`ApiError`]s, like every
//! other API failure.

use axum::extract::{
    FromRequest, FromRequestParts,
    rejection::{JsonRejection, PathRejection, QueryRejection},
};

use crate::error::ApiError;

/// A JSON request body.
#[derive(Debug, FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct JsonBody<T>(pub T);

/// Typed query parameters.
#[derive(Debug, FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct QueryParam<T>(pub T);

/// Typed path parameters.
#[derive(Debug, FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct PathParam<T>(pub T);

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::new(rejection.status(), "invalid_body", rejection.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejection: PathRejection) -> Self {
        Self::new(rejection.status(), "invalid_path", rejection.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::new(rejection.status(), "invalid_query", rejection.body_text())
    }
}
