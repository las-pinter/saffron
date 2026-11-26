//! Security headers and the request-origin check.

mod common;

use axum::http::StatusCode;
use common::{ADMIN_PASSWORD, TestApp};
use saffron_server::security::CONTENT_SECURITY_POLICY_VALUE;
use serde_json::json;

#[tokio::test]
async fn api_responses_carry_the_security_headers() {
    let app = TestApp::new().await;

    let reply = app.get("/api/health").send().await;

    assert_eq!(
        reply.header("content-security-policy"),
        Some(CONTENT_SECURITY_POLICY_VALUE)
    );
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(reply.header("x-frame-options"), Some("DENY"));
    assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(
        reply.header("cross-origin-opener-policy"),
        Some("same-origin")
    );
    assert!(reply.header("permissions-policy").is_some());
}

#[tokio::test]
async fn web_app_responses_carry_the_security_headers() {
    let app = TestApp::new().await;

    let reply = app.get("/holdings").send().await;

    assert_eq!(
        reply.header("content-security-policy"),
        Some(CONTENT_SECURITY_POLICY_VALUE)
    );
}

#[tokio::test]
async fn the_policy_allows_only_same_origin_scripts() {
    assert!(CONTENT_SECURITY_POLICY_VALUE.contains("script-src 'self';"));
    assert!(CONTENT_SECURITY_POLICY_VALUE.contains("frame-ancestors 'none'"));
}

#[tokio::test]
async fn https_responses_require_https_for_a_year() {
    let app = TestApp::with_origin("https://saffron.example.test").await;

    let reply = app.get("/api/health").send().await;

    assert_eq!(
        reply.header("strict-transport-security"),
        Some("max-age=31536000")
    );
}

#[tokio::test]
async fn localhost_responses_do_not_send_hsts() {
    let app = TestApp::new().await;

    let reply = app.get("/api/health").send().await;

    assert_eq!(reply.header("strict-transport-security"), None);
}

#[tokio::test]
async fn api_responses_are_never_cached() {
    let app = TestApp::new().await;

    let reply = app.get("/api/auth/state").send().await;

    assert_eq!(reply.header("cache-control"), Some("no-store"));
}

#[tokio::test]
async fn a_post_from_a_foreign_origin_is_rejected() {
    let app = TestApp::new().await;
    app.setup_admin().await;

    let reply = app
        .post("/api/auth/login")
        .origin(Some("https://evil.test"))
        .json(json!({ "username": "admin", "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::FORBIDDEN, json!("origin_rejected"))
    );
    assert_eq!(reply.header("set-cookie"), None);
}

#[tokio::test]
async fn a_post_without_origin_metadata_is_rejected() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/logout")
        .cookie(&cookie)
        .origin(None)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_post_marked_same_origin_by_fetch_metadata_is_allowed() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/logout")
        .cookie(&cookie)
        .origin(None)
        .header("sec-fetch-site", "same-origin")
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_post_marked_cross_site_by_fetch_metadata_is_rejected() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/logout")
        .cookie(&cookie)
        .origin(None)
        .header("sec-fetch-site", "cross-site")
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_get_without_origin_is_allowed() {
    let app = TestApp::new().await;

    let reply = app.get("/api/auth/state").origin(None).send().await;

    assert_eq!(reply.status, StatusCode::OK);
}
