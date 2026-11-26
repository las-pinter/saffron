//! Passkey steps shared by the passkey test files: registering, signing in, and
//! turning TOTP on.

use axum::http::StatusCode;
use jiff::SignedDuration;
use serde_json::{Value, json};
use totp_rs::{Algorithm, Builder, Secret};

use super::{ADMIN_PASSWORD, LOCAL_ORIGIN, Reply, TestApp, authenticator::SoftAuthenticator};

pub async fn start_registration(app: &TestApp, cookie: &str, password: &str) -> Reply {
    app.post("/api/auth/passkeys/register/start")
        .cookie(cookie)
        .json(json!({ "password": password }))
        .send()
        .await
}

pub async fn finish_registration(
    app: &TestApp,
    cookie: &str,
    ceremony_id: &Value,
    credential: Value,
) -> Reply {
    app.post("/api/auth/passkeys/register/finish")
        .cookie(cookie)
        .json(json!({ "ceremony_id": ceremony_id, "name": "Laptop", "credential": credential }))
        .send()
        .await
}

/// Registers `authenticator` for the session `cookie` signed in with `password`.
pub async fn register(
    app: &TestApp,
    cookie: &str,
    password: &str,
    authenticator: &mut SoftAuthenticator,
) -> Reply {
    let started = start_registration(app, cookie, password).await.json();
    let credential = authenticator.register(&started["options"]);
    finish_registration(app, cookie, &started["ceremony_id"], credential).await
}

/// The administrator with one registered passkey; returns the session cookie.
pub async fn admin_with_passkey(app: &TestApp) -> (String, SoftAuthenticator) {
    let cookie = app.setup_admin().await;
    let mut authenticator = SoftAuthenticator::new(LOCAL_ORIGIN);
    let reply = register(app, &cookie, ADMIN_PASSWORD, &mut authenticator).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    (cookie, authenticator)
}

pub async fn sign_in(app: &TestApp, authenticator: &mut SoftAuthenticator) -> Reply {
    let started = app
        .post("/api/auth/passkey/login/start")
        .send()
        .await
        .json();
    let credential = authenticator.sign_in(&started["options"]);
    app.post("/api/auth/passkey/login/finish")
        .json(json!({ "ceremony_id": started["ceremony_id"], "credential": credential }))
        .send()
        .await
}

pub async fn list(app: &TestApp, cookie: &str) -> Value {
    app.get("/api/auth/passkeys")
        .cookie(cookie)
        .send()
        .await
        .json()
}

pub async fn turn_on_totp(app: &TestApp, cookie: &str) -> String {
    let offer = app
        .post("/api/auth/totp/setup")
        .cookie(cookie)
        .json(json!({ "password": ADMIN_PASSWORD }))
        .send()
        .await
        .json();
    let secret = offer["secret"].as_str().expect("secret").to_owned();
    let reply = app
        .post("/api/auth/totp/confirm")
        .cookie(cookie)
        .json(json!({ "code": totp_code(app, &secret) }))
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    app.advance(SignedDuration::from_secs(30));
    secret
}

pub fn totp_code(app: &TestApp, secret: &str) -> String {
    let totp = Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_secret(Secret::try_from_base32(secret).expect("base32"))
        .build()
        .expect("generator");
    let now = u64::try_from(app.state.clock.now().as_second()).expect("positive time");
    totp.generate(now).to_string()
}

pub fn error(reply: &Reply) -> (StatusCode, Value) {
    (reply.status, reply.json()["error"].clone())
}

pub fn base64_url(bytes: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(bytes)
}
