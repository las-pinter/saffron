//! Two-factor sign-in with TOTP over HTTP: enrolment, login, replay, lockout, and
//! turning it off.

mod common;

use axum::http::StatusCode;
use common::{ADMIN_PASSWORD, TestApp, USER_PASSWORD};
use jiff::SignedDuration;
use serde_json::{Value, json};
use totp_rs::{Algorithm, Builder, Secret};

/// The code an authenticator app shows for `secret` at the app's current time.
fn code(app: &TestApp, secret: &str) -> String {
    let totp = Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_secret(Secret::try_from_base32(secret).expect("base32 secret"))
        .build()
        .expect("generator");
    let now = u64::try_from(app.state.clock.now().as_second()).expect("positive time");
    totp.generate(now).to_string()
}

fn next_step(app: &TestApp) {
    app.advance(SignedDuration::from_secs(30));
}

async fn start_setup(app: &TestApp, cookie: &str) -> Value {
    let reply = app
        .post("/api/auth/totp/setup")
        .cookie(cookie)
        .json(json!({ "password": ADMIN_PASSWORD }))
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::OK, "setup: {:?}", reply.json());
    reply.json()
}

/// An administrator with TOTP on; returns the session cookie and the secret.
async fn admin_with_totp(app: &TestApp) -> (String, String) {
    let cookie = app.setup_admin().await;
    let secret = start_setup(app, &cookie).await["secret"]
        .as_str()
        .expect("secret")
        .to_owned();
    let reply = app
        .post("/api/auth/totp/confirm")
        .cookie(&cookie)
        .json(json!({ "code": code(app, &secret) }))
        .send()
        .await;
    assert_eq!(
        reply.status,
        StatusCode::NO_CONTENT,
        "confirm: {:?}",
        reply.json()
    );
    (cookie, secret)
}

async fn login_with_code(app: &TestApp, code: Option<&str>) -> common::Reply {
    app.post("/api/auth/login")
        .json(json!({ "username": "admin", "password": ADMIN_PASSWORD, "totp_code": code }))
        .send()
        .await
}

async fn totp_enabled(app: &TestApp, cookie: &str) -> Value {
    app.get("/api/auth/state")
        .cookie(cookie)
        .send()
        .await
        .json()["user"]["totp_enabled"]
        .clone()
}

#[tokio::test]
async fn setup_needs_the_current_password() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/totp/setup")
        .cookie(&cookie)
        .json(json!({ "password": "wrong password 1" }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::FORBIDDEN, json!("wrong_password"))
    );
}

#[tokio::test]
async fn setup_offers_a_secret_and_an_otpauth_url() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let offer = start_setup(&app, &cookie).await;

    let secret = offer["secret"].as_str().expect("secret");
    assert!(Secret::try_from_base32(secret).is_ok(), "{secret}");
    let url = offer["otpauth_url"].as_str().expect("url");
    assert!(url.starts_with("otpauth://totp/Saffron:admin?"), "{url}");
}

#[tokio::test]
async fn totp_stays_off_until_a_code_confirms_it() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    start_setup(&app, &cookie).await;

    let reply = login_with_code(&app, None).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(totp_enabled(&app, &cookie).await, json!(false));
}

#[tokio::test]
async fn confirm_with_a_wrong_code_is_rejected() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    start_setup(&app, &cookie).await;

    let reply = app
        .post("/api/auth/totp/confirm")
        .cookie(&cookie)
        .json(json!({ "code": "000000" }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("invalid_code"))
    );
}

#[tokio::test]
async fn confirm_without_setup_is_rejected() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = app
        .post("/api/auth/totp/confirm")
        .cookie(&cookie)
        .json(json!({ "code": "123456" }))
        .send()
        .await;

    assert_eq!(reply.json()["error"], json!("no_totp_pending"));
}

#[tokio::test]
async fn confirm_turns_totp_on() {
    let app = TestApp::new().await;

    let (cookie, _) = admin_with_totp(&app).await;

    assert_eq!(totp_enabled(&app, &cookie).await, json!(true));
}

#[tokio::test]
async fn the_secret_is_stored_sealed() {
    let app = TestApp::new().await;
    let (_, secret) = admin_with_totp(&app).await;

    let stored: String = sqlx::query_scalar("SELECT totp_secret FROM users")
        .fetch_one(&app.state.pool)
        .await
        .expect("stored secret");

    assert!(stored.starts_with("v1:"), "{stored}");
    assert!(!stored.contains(&secret), "{stored}");
}

#[tokio::test]
async fn setup_while_totp_is_on_is_a_conflict() {
    let app = TestApp::new().await;
    let (cookie, _) = admin_with_totp(&app).await;

    let reply = app
        .post("/api/auth/totp/setup")
        .cookie(&cookie)
        .json(json!({ "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_without_a_code_asks_for_one() {
    let app = TestApp::new().await;
    admin_with_totp(&app).await;

    let reply = login_with_code(&app, None).await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::UNAUTHORIZED, json!("totp_required"))
    );
    assert_eq!(reply.header("set-cookie"), None);
}

#[tokio::test]
async fn a_wrong_password_never_reaches_the_code_prompt() {
    let app = TestApp::new().await;
    admin_with_totp(&app).await;

    let reply = app.login("admin", "wrong password 1").await;

    assert_eq!(reply.json()["error"], json!("invalid_credentials"));
}

#[tokio::test]
async fn login_with_the_right_code_signs_in() {
    let app = TestApp::new().await;
    let (_, secret) = admin_with_totp(&app).await;
    next_step(&app);

    let reply = login_with_code(&app, Some(&code(&app, &secret))).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.session_cookie().is_some());
}

#[tokio::test]
async fn login_with_a_wrong_code_is_refused() {
    let app = TestApp::new().await;
    admin_with_totp(&app).await;
    next_step(&app);

    let reply = login_with_code(&app, Some("000000")).await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::UNAUTHORIZED, json!("invalid_code"))
    );
}

#[tokio::test]
async fn a_code_is_accepted_only_once() {
    let app = TestApp::new().await;
    let (_, secret) = admin_with_totp(&app).await;
    next_step(&app);
    let used = code(&app, &secret);
    login_with_code(&app, Some(&used)).await;

    let reply = login_with_code(&app, Some(&used)).await;

    assert_eq!(reply.json()["error"], json!("invalid_code"));
}

#[tokio::test]
async fn the_confirming_code_cannot_sign_in() {
    let app = TestApp::new().await;
    let (_, secret) = admin_with_totp(&app).await;

    let reply = login_with_code(&app, Some(&code(&app, &secret))).await;

    assert_eq!(reply.json()["error"], json!("invalid_code"));
}

#[tokio::test]
async fn wrong_codes_count_toward_the_lockout() {
    let app = TestApp::new().await;
    let (_, secret) = admin_with_totp(&app).await;
    next_step(&app);
    for _ in 0..5 {
        login_with_code(&app, Some("000000")).await;
    }

    let reply = login_with_code(&app, Some(&code(&app, &secret))).await;

    assert_eq!(reply.json()["error"], json!("account_locked"));
}

#[tokio::test]
async fn disable_needs_the_current_password() {
    let app = TestApp::new().await;
    let (cookie, _) = admin_with_totp(&app).await;

    let reply = app
        .post("/api/auth/totp/disable")
        .cookie(&cookie)
        .json(json!({ "password": "wrong password 1" }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(totp_enabled(&app, &cookie).await, json!(true));
}

#[tokio::test]
async fn disable_turns_totp_off() {
    let app = TestApp::new().await;
    let (cookie, _) = admin_with_totp(&app).await;

    app.post("/api/auth/totp/disable")
        .cookie(&cookie)
        .json(json!({ "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(login_with_code(&app, None).await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_administrator_cannot_reset_their_own_totp() {
    let app = TestApp::new().await;
    let (admin, _) = admin_with_totp(&app).await;
    let me = app
        .get("/api/auth/state")
        .cookie(&admin)
        .send()
        .await
        .json()["user"]["id"]
        .clone();

    let reply = app
        .post(&format!("/api/users/{me}/totp/reset"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::BAD_REQUEST, json!("own_totp"))
    );
    assert_eq!(totp_enabled(&app, &admin).await, json!(true));
}

#[tokio::test]
async fn resetting_another_users_totp_signs_them_out_and_lets_them_in_without_a_code() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");
    let offer = app
        .post("/api/auth/totp/setup")
        .cookie(&member)
        .json(json!({ "password": USER_PASSWORD }))
        .send()
        .await
        .json();
    let secret = offer["secret"].as_str().expect("secret").to_owned();
    app.post("/api/auth/totp/confirm")
        .cookie(&member)
        .json(json!({ "code": code(&app, &secret) }))
        .send()
        .await;
    let id = app
        .get("/api/auth/state")
        .cookie(&member)
        .send()
        .await
        .json()["user"]["id"]
        .clone();

    let reply = app
        .post(&format!("/api/users/{id}/totp/reset"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let old_session = app.get("/api/auth/state").cookie(&member).send().await;
    assert_eq!(old_session.json()["user"], Value::Null);
    assert_eq!(
        app.login("member", USER_PASSWORD).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_regular_user_cannot_reset_totp() {
    let app = TestApp::new().await;
    let (admin, _) = admin_with_totp(&app).await;
    app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    let reply = app
        .post("/api/users/1/totp/reset")
        .cookie(&member)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}
