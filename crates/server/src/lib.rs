//! The Saffron server: HTTP API and the embedded web application.

pub mod auth;
pub mod book;
pub mod clock;
pub mod error;
pub mod exports;
pub mod fx_refresh;
mod health;
pub mod healthcheck;
pub mod imports;
pub mod instruments;
pub mod performance;
pub mod portfolios;
pub mod prices;
pub mod refresh;
pub mod request;
pub mod secrets;
pub mod security;
pub mod settings;
pub mod state;
mod static_files;
pub mod targets;
pub mod users;

use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{any, delete, get, post, put},
};

pub use health::HealthResponse;
pub use state::AppState;

/// Builds the HTTP router: the API under `/api`, the web application for every other
/// GET or HEAD request, and security headers on every response.
pub fn app(state: AppState) -> Router {
    Router::new()
        .nest("/api", api(state.clone()))
        // A nested router does not match the trailing-slash form of its own prefix.
        .route("/api/", any(api_not_found))
        // `get` also answers HEAD; other methods on non-API paths get 405.
        .fallback_service(get(static_files::spa))
        .layer(middleware::from_fn_with_state(state, security::headers))
}

/// Every path under `/api`, including `/api` itself, stays in this router, so API
/// clients never receive `index.html`.
fn api(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/auth/state", get(auth::routes::state))
        .route("/auth/setup", post(auth::routes::setup))
        .route("/auth/login", post(auth::routes::login))
        .route("/auth/logout", post(auth::routes::logout))
        .route("/auth/password", post(auth::routes::change_password))
        .route(
            "/auth/passkey/login/start",
            post(auth::passkey_login::login_start),
        )
        .route(
            "/auth/passkey/login/finish",
            post(auth::passkey_login::login_finish),
        )
        .route("/auth/passkeys", get(auth::passkeys::list))
        .route("/auth/passkeys/{id}", delete(auth::passkeys::delete))
        .route(
            "/auth/passkeys/register/start",
            post(auth::passkeys::register_start),
        )
        .route(
            "/auth/passkeys/register/finish",
            post(auth::passkeys::register_finish),
        )
        .route("/auth/totp/setup", post(auth::totp::setup))
        .route("/auth/totp/confirm", post(auth::totp::confirm))
        .route("/auth/totp/disable", post(auth::totp::disable))
        .route(
            "/portfolios",
            get(portfolios::list).post(portfolios::create),
        )
        .nest(
            "/portfolios/{portfolio_id}",
            portfolios::one_portfolio(state.clone()),
        )
        .route(
            "/memberships/{portfolio_id}",
            delete(portfolios::shares::leave),
        )
        .route("/instruments", post(instruments::create))
        .route("/instruments/lookup", get(instruments::lookup))
        .route("/instruments/{id}", get(instruments::show))
        .route("/instruments/{id}/prices", get(prices::history))
        .route("/instruments/{id}/prices/{date}", put(prices::enter))
        .route(
            "/instruments/{id}/classification",
            put(instruments::classify),
        )
        .route("/settings", get(settings::show).patch(settings::update))
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}/disable", post(users::disable))
        .route("/users/{id}/enable", post(users::enable))
        .route("/users/{id}/password", post(users::reset_password))
        .route("/users/{id}/totp/reset", post(auth::totp::reset))
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::guard_api,
        ))
        .with_state(state)
}

#[allow(clippy::unused_async)]
async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
