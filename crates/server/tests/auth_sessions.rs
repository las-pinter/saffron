//! Session lifetime over HTTP: idle and absolute expiry, logout, cookies, and
//! cleanup.

mod common;

use axum::http::StatusCode;
use common::{ADMIN_PASSWORD, TestApp};
use jiff::SignedDuration;
use serde_json::json;

async fn signed_in_user(app: &TestApp, cookie: &str) -> serde_json::Value {
    app.get("/api/auth/state")
        .cookie(cookie)
        .send()
        .await
        .json()["user"]
        .clone()
}

fn hours(count: i64) -> SignedDuration {
    SignedDuration::from_hours(count)
}

#[tokio::test]
async fn a_session_of_a_disabled_user_is_refused() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let id = signed_in_user(&app, &cookie).await["id"]
        .as_i64()
        .expect("id");

    // Disable directly, leaving the session row in place.
    saffron_db::users::set_disabled(&app.state.pool, id, Some(jiff::Timestamp::now()))
        .await
        .expect("disable");

    assert_eq!(signed_in_user(&app, &cookie).await, json!(null));
}

#[tokio::test]
async fn starting_a_session_sweeps_ended_ones() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    app.advance(hours(13));

    app.login("admin", ADMIN_PASSWORD).await;

    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&app.state.pool)
        .await
        .expect("count");
    assert_eq!(sessions, 1);
}

#[tokio::test]
async fn logout_ends_the_session_and_clears_the_cookie() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app.post("/api/auth/logout").cookie(&cookie).send().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(
        reply
            .header("set-cookie")
            .is_some_and(|value| value.contains("Max-Age=0")),
        "set-cookie: {:?}",
        reply.header("set-cookie")
    );
    assert_eq!(signed_in_user(&app, &cookie).await, json!(null));
}

#[tokio::test]
async fn a_session_ends_after_twelve_idle_hours() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    app.advance(hours(12));

    assert_eq!(signed_in_user(&app, &cookie).await, json!(null));
}

#[tokio::test]
async fn activity_keeps_a_session_alive_past_twelve_hours() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    app.advance(hours(11));
    signed_in_user(&app, &cookie).await;

    app.advance(hours(11));

    assert_eq!(
        signed_in_user(&app, &cookie).await["username"],
        json!("admin")
    );
}

#[tokio::test]
async fn a_session_ends_after_thirty_days_despite_activity() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    for _ in 0..65 {
        app.advance(hours(11));
        signed_in_user(&app, &cookie).await;
    }

    // 66 × 11 h = 726 h, past the 720 h limit.
    app.advance(hours(11));

    assert_eq!(signed_in_user(&app, &cookie).await, json!(null));
}

#[tokio::test]
async fn a_session_cookie_lasts_thirty_days_and_is_http_only() {
    let app = TestApp::new().await;
    let token = app.state.setup.issue().expect("token");

    let reply = app
        .post("/api/auth/setup")
        .json(json!({
            "token": token,
            "username": "admin",
            "display_name": "Admin",
            "password": ADMIN_PASSWORD,
        }))
        .send()
        .await;

    let set_cookie = reply.header("set-cookie").expect("cookie");
    assert!(set_cookie.starts_with("saffron_session="), "{set_cookie}");
    assert!(set_cookie.contains("; HttpOnly"), "{set_cookie}");
    assert!(set_cookie.contains("; Max-Age=2592000"), "{set_cookie}");
}

#[tokio::test]
async fn an_https_origin_uses_a_secure_host_cookie() {
    let app = TestApp::with_origin("https://saffron.example.test").await;

    let cookie = app.setup_admin().await;

    assert!(cookie.starts_with("__Host-saffron_session="), "{cookie}");
}
