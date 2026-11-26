//! User administration over HTTP.

mod common;

use axum::http::StatusCode;
use common::{TestApp, USER_PASSWORD};
use serde_json::json;

#[tokio::test]
async fn listing_users_without_a_session_is_unauthenticated() {
    let app = TestApp::new().await;

    let reply = app.get("/api/users").send().await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_regular_user_cannot_list_users() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    let reply = app.get("/api/users").cookie(&member).send().await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::FORBIDDEN, json!("admin_required"))
    );
}

#[tokio::test]
async fn a_regular_user_cannot_create_users() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    let reply = app
        .post("/api/users")
        .cookie(&member)
        .json(json!({ "username": "x", "display_name": "x", "password": USER_PASSWORD }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn created_users_are_listed_without_secrets() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let member = app.create_user(&admin, "member").await;

    let reply = app.get("/api/users").cookie(&admin).send().await;

    assert_eq!(
        reply.json()[1],
        json!({
            "id": member,
            "username": "member",
            "display_name": "member",
            "is_admin": false,
            "disabled": false,
        })
    );
}

#[tokio::test]
async fn a_taken_username_is_a_conflict_ignoring_case() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;

    let reply = app
        .post("/api/users")
        .cookie(&admin)
        .json(json!({ "username": "MEMBER", "display_name": "x", "password": USER_PASSWORD }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::CONFLICT, json!("username_taken"))
    );
}

#[tokio::test]
async fn an_invalid_username_is_rejected() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;

    let reply = app
        .post("/api/users")
        .cookie(&admin)
        .json(json!({ "username": "two words", "display_name": "x", "password": USER_PASSWORD }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("invalid_username"))
    );
}

#[tokio::test]
async fn disabling_a_user_ends_their_sessions() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let id = app.create_user(&admin, "member").await;
    app.login("member", USER_PASSWORD).await;

    let reply = app
        .post(&format!("/api/users/{id}/disable"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = ?")
        .bind(id)
        .fetch_one(&app.state.pool)
        .await
        .expect("count sessions");
    assert_eq!(sessions, 0);
}

#[tokio::test]
async fn an_administrator_cannot_disable_themselves() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let me = app
        .get("/api/auth/state")
        .cookie(&admin)
        .send()
        .await
        .json()["user"]["id"]
        .clone();

    let reply = app
        .post(&format!("/api/users/{me}/disable"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("cannot_disable_self"))
    );
}

#[tokio::test]
async fn enabling_a_user_lets_them_sign_in_again() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let id = app.create_user(&admin, "member").await;
    app.post(&format!("/api/users/{id}/disable"))
        .cookie(&admin)
        .send()
        .await;

    app.post(&format!("/api/users/{id}/enable"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(
        app.login("member", USER_PASSWORD).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn enabling_a_user_lifts_a_sign_in_lock() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let id = app.create_user(&admin, "member").await;
    for _ in 0..5 {
        app.login("member", "wrong password 1").await;
    }

    app.post(&format!("/api/users/{id}/enable"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(
        app.login("member", USER_PASSWORD).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_password_reset_replaces_the_password_and_ends_sessions() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let id = app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    let reply = app
        .post(&format!("/api/users/{id}/password"))
        .cookie(&admin)
        .json(json!({ "password": "reset password 1" }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let state = app
        .get("/api/auth/state")
        .cookie(&member)
        .send()
        .await
        .json();
    assert_eq!(state["user"], json!(null));
    assert_eq!(
        app.login("member", "reset password 1").await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_unknown_user_id_is_not_found() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;

    let reply = app
        .post("/api/users/404/enable")
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_non_numeric_user_id_is_a_json_error() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;

    let reply = app
        .post("/api/users/abc/enable")
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("invalid_path"))
    );
}
