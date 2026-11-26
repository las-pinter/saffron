//! Response hardening: security headers on every response, and the request-origin
//! check that stops cross-site request forgery on the API.

use axum::{
    extract::{Request, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, ORIGIN, REFERRER_POLICY,
            STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{auth::origin::PublicOrigin, error::ApiError, state::AppState};

/// Everything loads from Saffron itself; nothing may frame it.
pub const CONTENT_SECURITY_POLICY_VALUE: &str = "default-src 'self'; script-src 'self'; \
     style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; \
     object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

const PERMISSIONS_POLICY_VALUE: &str = "camera=(), microphone=(), geolocation=(), payment=()";
const HSTS_VALUE: &str = "max-age=31536000";

/// Adds the security headers to every response.
pub async fn headers(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let fixed: [(HeaderName, &'static str); 6] = [
        (CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY_VALUE),
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (X_FRAME_OPTIONS, "DENY"),
        (REFERRER_POLICY, "no-referrer"),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
        (
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS_POLICY_VALUE,
        ),
    ];
    for (name, value) in fixed {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if state.origin.is_https() {
        headers.insert(
            STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS_VALUE),
        );
    }
    response
}

/// Refuses state-changing API requests that did not come from the public origin,
/// and keeps API responses out of every cache.
pub async fn guard_api(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let mut response = if safe || same_origin(&state.origin, request.headers()) {
        next.run(request).await
    } else {
        ApiError::forbidden(
            "origin_rejected",
            "the request did not come from the Saffron origin",
        )
        .into_response()
    };
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Browsers send `Origin` on every cross-site and every `POST` request; a request
/// without it passes only when `Sec-Fetch-Site` says it is same-origin.
fn same_origin(origin: &PublicOrigin, headers: &HeaderMap) -> bool {
    match headers.get(ORIGIN).and_then(|value| value.to_str().ok()) {
        Some(candidate) => origin.accepts(candidate),
        None => {
            headers
                .get("sec-fetch-site")
                .and_then(|value| value.to_str().ok())
                == Some("same-origin")
        }
    }
}
