//! The Saffron server: HTTP API and the embedded web application.

mod health;
mod static_files;

use axum::{
    Router,
    http::StatusCode,
    routing::{any, get},
};

pub use health::HealthResponse;

/// Builds the HTTP router: the API under `/api`, the web application for every other
/// GET or HEAD request.
pub fn app() -> Router {
    Router::new()
        .nest("/api", api())
        // A nested router does not match the trailing-slash form of its own prefix.
        .route("/api/", any(api_not_found))
        // `get` also answers HEAD; other methods on non-API paths get 405.
        .fallback_service(get(static_files::spa))
}

/// Every path under `/api`, including `/api` itself, stays in this router, so API
/// clients never receive `index.html`.
fn api() -> Router {
    Router::new()
        .route("/health", get(health::health))
        .fallback(api_not_found)
}

#[allow(clippy::unused_async)]
async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
