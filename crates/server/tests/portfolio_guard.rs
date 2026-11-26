//! The portfolio access guard on routes of every role, through probe routes behind
//! the same layer the real portfolio routes use. Task 7's own writes are all
//! owner-only, so these probes are what show the guard's own rules.

mod common;

use axum::{Router, http::StatusCode, routing::get};
use common::{TestApp, USER_PASSWORD};
use saffron_server::portfolios::{guard::PortfolioAccess, guarded};
use serde_json::{Value, json};

async fn role(access: PortfolioAccess) -> axum::Json<&'static str> {
    axum::Json(access.0.role.as_str())
}

/// The app plus `GET` and `POST /probe/{portfolio_id}` behind the guard, an owner
/// with one portfolio, and a member signed in; returns the member cookie and id.
async fn probed(member_role: Option<&str>) -> (TestApp, String, Value) {
    let app = TestApp::new().await;
    let probe = guarded(
        Router::new().route("/", get(role).post(role)),
        app.state.clone(),
    );
    let extra = Router::new()
        .nest("/probe/{portfolio_id}", probe)
        .with_state(app.state.clone());
    let app = app.with_routes(extra);
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;
    let id = app
        .post("/api/portfolios")
        .cookie(&admin)
        .json(json!({ "name": "Main" }))
        .send()
        .await
        .json()["id"]
        .clone();
    if let Some(role) = member_role {
        app.post(&format!("/api/portfolios/{id}/shares"))
            .cookie(&admin)
            .json(json!({ "username": "member", "role": role }))
            .send()
            .await;
    }
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");
    (app, member, id)
}

#[tokio::test]
async fn a_viewer_passes_the_guard_to_read() {
    let (app, member, id) = probed(Some("viewer")).await;

    let reply = app
        .get(&format!("/probe/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()),
        (StatusCode::OK, json!("viewer"))
    );
}

#[tokio::test]
async fn a_viewer_is_stopped_by_the_guard_on_a_write() {
    let (app, member, id) = probed(Some("viewer")).await;

    let reply = app
        .post(&format!("/probe/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::FORBIDDEN, json!("insufficient_role"))
    );
}

#[tokio::test]
async fn an_editor_passes_the_guard_to_write() {
    let (app, member, id) = probed(Some("editor")).await;

    let reply = app
        .post(&format!("/probe/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()),
        (StatusCode::OK, json!("editor"))
    );
}

#[tokio::test]
async fn a_user_without_access_never_reaches_the_handler() {
    let (app, member, id) = probed(None).await;

    let read = app
        .get(&format!("/probe/{id}"))
        .cookie(&member)
        .send()
        .await;
    let write = app
        .post(&format!("/probe/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(
        (read.status, write.status),
        (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND)
    );
}
