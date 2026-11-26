//! Sign-in flows over HTTP: setup, login, the audit log, and password change.

mod common;

use axum::http::StatusCode;
use common::{ADMIN_PASSWORD, TestApp, USER_PASSWORD};
use serde_json::json;

async fn signed_in_user(app: &TestApp, cookie: &str) -> serde_json::Value {
    app.get("/api/auth/state")
        .cookie(cookie)
        .send()
        .await
        .json()["user"]
        .clone()
}

#[tokio::test]
async fn a_fresh_server_requires_setup() {
    let app = TestApp::new().await;

    let reply = app.get("/api/auth/state").send().await;

    assert_eq!(
        reply.json(),
        json!({ "setup_required": true, "user": null })
    );
}

#[tokio::test]
async fn setup_with_a_wrong_token_is_forbidden() {
    let app = TestApp::new().await;
    app.state.setup.issue().expect("token");

    let reply = app
        .post("/api/auth/setup")
        .json(json!({
            "token": "guessed",
            "username": "admin",
            "display_name": "Admin",
            "password": ADMIN_PASSWORD,
        }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::FORBIDDEN, json!("invalid_setup_token"))
    );
}

#[tokio::test]
async fn setup_creates_a_signed_in_administrator() {
    let app = TestApp::new().await;

    let cookie = app.setup_admin().await;

    let state = app
        .get("/api/auth/state")
        .cookie(&cookie)
        .send()
        .await
        .json();
    assert_eq!(state["setup_required"], json!(false));
    assert_eq!(state["user"]["username"], json!("admin"));
    assert_eq!(state["user"]["is_admin"], json!(true));
}

#[tokio::test]
async fn setup_after_a_user_exists_is_a_conflict() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    let token = app.state.setup.issue().expect("token");

    let reply = app
        .post("/api/auth/setup")
        .json(json!({
            "token": token,
            "username": "second",
            "display_name": "Second",
            "password": ADMIN_PASSWORD,
        }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_setup_token_works_only_once() {
    let app = TestApp::new().await;
    let token = app.state.setup.issue().expect("token");
    let body = json!({
        "token": token,
        "username": "admin",
        "display_name": "Admin",
        "password": ADMIN_PASSWORD,
    });
    app.post("/api/auth/setup").json(body.clone()).send().await;

    let reply = app.post("/api/auth/setup").json(body).send().await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn setup_rejects_a_short_password() {
    let app = TestApp::new().await;
    let token = app.state.setup.issue().expect("token");

    let reply = app
        .post("/api/auth/setup")
        .json(json!({
            "token": token,
            "username": "admin",
            "display_name": "Admin",
            "password": "short",
        }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("invalid_password"))
    );
}

#[tokio::test]
async fn login_with_the_right_password_starts_a_session() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;

    let reply = app.login("member", USER_PASSWORD).await;

    assert_eq!(reply.status, StatusCode::OK);
    let cookie = reply.session_cookie().expect("cookie");
    assert_eq!(
        signed_in_user(&app, &cookie).await["username"],
        json!("member")
    );
}

#[tokio::test]
async fn login_ignores_username_case_and_spaces() {
    let app = TestApp::new().await;
    app.setup_admin().await;

    let reply = app.login(" ADMIN ", ADMIN_PASSWORD).await;

    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn login_with_a_wrong_password_is_refused() {
    let app = TestApp::new().await;
    app.setup_admin().await;

    let reply = app.login("admin", "wrong password 1").await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::UNAUTHORIZED, json!("invalid_credentials"))
    );
    assert_eq!(reply.header("set-cookie"), None);
}

#[tokio::test]
async fn an_unknown_username_gets_the_same_answer_as_a_wrong_password() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    let wrong_password = app.login("admin", "wrong password 1").await;

    let unknown_user = app.login("nobody", "wrong password 1").await;

    assert_eq!(
        (unknown_user.status, unknown_user.json()),
        (wrong_password.status, wrong_password.json())
    );
}

#[tokio::test]
async fn a_disabled_user_cannot_log_in() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let member = app.create_user(&admin, "member").await;
    app.post(&format!("/api/users/{member}/disable"))
        .cookie(&admin)
        .send()
        .await;

    let reply = app.login("member", USER_PASSWORD).await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_malformed_body_is_a_json_error() {
    let app = TestApp::new().await;

    let reply = app
        .post("/api/auth/login")
        .raw_json("{not json")
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("invalid_body"))
    );
}

#[tokio::test]
async fn login_is_written_to_the_audit_log() {
    let app = TestApp::new().await;
    app.setup_admin().await;

    app.login("admin", ADMIN_PASSWORD).await;

    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
        .fetch_all(&app.state.pool)
        .await
        .expect("audit rows");
    assert_eq!(actions, ["auth.setup", "auth.login"]);
}

#[tokio::test]
async fn change_password_with_a_wrong_current_password_is_forbidden() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/password")
        .cookie(&cookie)
        .json(json!({ "current_password": "wrong password 1", "new_password": "new password 12" }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::FORBIDDEN, json!("wrong_password"))
    );
}

#[tokio::test]
async fn change_password_replaces_the_password() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/password")
        .cookie(&cookie)
        .json(json!({ "current_password": ADMIN_PASSWORD, "new_password": "new password 12" }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.login("admin", ADMIN_PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.login("admin", "new password 12").await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn change_password_ends_other_sessions_but_keeps_this_one() {
    let app = TestApp::new().await;
    let this = app.setup_admin().await;
    let other = app
        .login("admin", ADMIN_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    app.post("/api/auth/password")
        .cookie(&this)
        .json(json!({ "current_password": ADMIN_PASSWORD, "new_password": "new password 12" }))
        .send()
        .await;

    assert_eq!(signed_in_user(&app, &other).await, json!(null));
    assert_eq!(
        signed_in_user(&app, &this).await["username"],
        json!("admin")
    );
}

#[tokio::test]
async fn a_correct_current_password_clears_earlier_failures() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    for _ in 0..4 {
        app.login("admin", "wrong password 1").await;
    }
    app.post("/api/auth/password")
        .cookie(&cookie)
        .json(json!({ "current_password": ADMIN_PASSWORD, "new_password": "new password 12" }))
        .send()
        .await;

    app.login("admin", "wrong password 1").await;

    assert_eq!(
        app.login("admin", "new password 12").await.status,
        StatusCode::OK
    );
}
