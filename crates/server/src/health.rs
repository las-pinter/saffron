//! The liveness endpoint.

use axum::Json;
use serde::Serialize;

/// Current health of the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthResponse {
    /// Always `ok` while the process serves requests.
    pub status: &'static str,
    /// Binary name.
    pub name: &'static str,
    /// Cargo package version.
    pub version: &'static str,
}

/// Reports that the server is alive.
// An axum handler must be async even when it has nothing to await.
#[allow(clippy::unused_async)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        name: "saffron",
        version: env!("CARGO_PKG_VERSION"),
    })
}
