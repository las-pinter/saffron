//! Portfolio steps shared by the portfolio test files: a household of two users,
//! creating and sharing portfolios, and calling any portfolio route.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use super::{Reply, TestApp, USER_PASSWORD};

/// Every route about one portfolio, with a body that would succeed for the owner.
pub fn portfolio_routes(id: &Value) -> Vec<(Method, String, Option<Value>)> {
    let base = format!("/api/portfolios/{id}");
    vec![
        (Method::GET, base.clone(), None),
        (
            Method::PATCH,
            base.clone(),
            Some(json!({ "name": "Renamed" })),
        ),
        (Method::POST, format!("{base}/archive"), None),
        (Method::POST, format!("{base}/unarchive"), None),
        (Method::GET, format!("{base}/shares"), None),
        (
            Method::POST,
            format!("{base}/shares"),
            Some(json!({ "username": "admin", "role": "viewer" })),
        ),
        (Method::DELETE, format!("{base}/shares/1"), None),
    ]
}

pub struct Household {
    pub app: TestApp,
    pub admin: String,
    pub member: String,
}

/// An administrator and a member, both signed in.
pub async fn household() -> Household {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;
    let member = sign_in(&app, "member").await;
    Household { app, admin, member }
}

pub async fn sign_in(app: &TestApp, username: &str) -> String {
    app.login(username, USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie")
}

pub async fn create(app: &TestApp, cookie: &str, body: Value) -> Reply {
    app.post("/api/portfolios")
        .cookie(cookie)
        .json(body)
        .send()
        .await
}

/// Creates a portfolio named `name` for `cookie`; returns its id.
pub async fn portfolio(app: &TestApp, cookie: &str, name: &str) -> Value {
    let reply = create(app, cookie, json!({ "name": name })).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    reply.json()["id"].clone()
}

pub async fn share(app: &TestApp, cookie: &str, id: &Value, username: &str, role: &str) -> Reply {
    app.post(&format!("/api/portfolios/{id}/shares"))
        .cookie(cookie)
        .json(json!({ "username": username, "role": role }))
        .send()
        .await
}

pub async fn call(app: &TestApp, cookie: &str, route: &(Method, String, Option<Value>)) -> Reply {
    let (method, path, body) = route;
    let call = app.call(method.clone(), path).cookie(cookie);
    match body {
        Some(body) => call.json(body.clone()).send().await,
        None => call.send().await,
    }
}

pub fn error(reply: &Reply) -> (StatusCode, Value) {
    (reply.status, reply.json()["error"].clone())
}
