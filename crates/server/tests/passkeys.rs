//! Passkey registration, listing, and removal over HTTP, with a software
//! authenticator.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    ADMIN_PASSWORD, LOCAL_ORIGIN, TestApp, USER_PASSWORD,
    authenticator::SoftAuthenticator,
    passkey_steps::{
        admin_with_passkey, base64_url, error, finish_registration, list, register,
        start_registration, totp_code, turn_on_totp,
    },
};
use serde_json::{Value, json};

#[tokio::test]
async fn registration_asks_for_a_discoverable_credential() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let started = start_registration(&app, &cookie, ADMIN_PASSWORD)
        .await
        .json();

    let selection = &started["options"]["publicKey"]["authenticatorSelection"];
    assert_eq!(
        (&selection["residentKey"], &selection["requireResidentKey"]),
        (&json!("required"), &json!(true))
    );
}

#[tokio::test]
async fn registration_needs_the_password() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;

    let reply = start_registration(&app, &cookie, "wrong password 1").await;

    assert_eq!(
        error(&reply),
        (StatusCode::FORBIDDEN, json!("wrong_password"))
    );
}

#[tokio::test]
async fn registration_needs_a_code_while_totp_is_on() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    turn_on_totp(&app, &cookie).await;

    let reply = start_registration(&app, &cookie, ADMIN_PASSWORD).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("totp_required"))
    );
}

#[tokio::test]
async fn registration_with_a_code_succeeds_while_totp_is_on() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let secret = turn_on_totp(&app, &cookie).await;

    let reply = app
        .post("/api/auth/passkeys/register/start")
        .cookie(&cookie)
        .json(json!({ "password": ADMIN_PASSWORD, "code": totp_code(&app, &secret) }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
}

#[tokio::test]
async fn a_passkey_is_registered_only_once() {
    let app = TestApp::new().await;
    let (cookie, mut authenticator) = admin_with_passkey(&app).await;

    let reply = register(&app, &cookie, ADMIN_PASSWORD, &mut authenticator).await;

    assert_eq!(
        error(&reply),
        (StatusCode::CONFLICT, json!("passkey_exists"))
    );
}

#[tokio::test]
async fn registration_excludes_the_users_passkeys() {
    let app = TestApp::new().await;
    let (cookie, authenticator) = admin_with_passkey(&app).await;

    let started = start_registration(&app, &cookie, ADMIN_PASSWORD)
        .await
        .json();

    let excluded = &started["options"]["publicKey"]["excludeCredentials"];
    assert_eq!(
        excluded[0]["id"],
        json!(base64_url(&authenticator.credential_id))
    );
}

#[tokio::test]
async fn a_passkey_name_must_not_be_blank() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let started = start_registration(&app, &cookie, ADMIN_PASSWORD)
        .await
        .json();
    let credential = SoftAuthenticator::new(LOCAL_ORIGIN).register(&started["options"]);

    let reply = app
        .post("/api/auth/passkeys/register/finish")
        .cookie(&cookie)
        .json(json!({ "ceremony_id": started["ceremony_id"], "name": "  ", "credential": credential }))
        .send()
        .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_name"))
    );
}

#[tokio::test]
async fn a_registration_ceremony_is_used_once() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let started = start_registration(&app, &cookie, ADMIN_PASSWORD)
        .await
        .json();
    let first = SoftAuthenticator::new(LOCAL_ORIGIN).register(&started["options"]);
    let second = SoftAuthenticator::new(LOCAL_ORIGIN).register(&started["options"]);
    finish_registration(&app, &cookie, &started["ceremony_id"], first).await;

    let reply = finish_registration(&app, &cookie, &started["ceremony_id"], second).await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("ceremony_expired"))
    );
}

#[tokio::test]
async fn a_registration_ceremony_belongs_to_its_session() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let other_session = app
        .login("admin", ADMIN_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");
    let started = start_registration(&app, &cookie, ADMIN_PASSWORD)
        .await
        .json();
    let credential = SoftAuthenticator::new(LOCAL_ORIGIN).register(&started["options"]);

    let reply =
        finish_registration(&app, &other_session, &started["ceremony_id"], credential).await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("ceremony_expired"))
    );
}

#[tokio::test]
async fn passkeys_are_listed_without_their_credentials() {
    let app = TestApp::new().await;
    let (cookie, _) = admin_with_passkey(&app).await;

    let listed = list(&app, &cookie).await;

    let first = listed[0].as_object().expect("passkey");
    let mut keys: Vec<_> = first.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["created_at", "id", "last_used_at", "name"]);
    assert_eq!(
        (&first["name"], &first["last_used_at"]),
        (&json!("Laptop"), &Value::Null)
    );
}

#[tokio::test]
async fn a_user_cannot_delete_another_users_passkey() {
    let app = TestApp::new().await;
    let (admin, _) = admin_with_passkey(&app).await;
    let id = list(&app, &admin).await[0]["id"].clone();
    app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    let reply = app
        .call(Method::DELETE, &format!("/api/auth/passkeys/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(list(&app, &admin).await.as_array().map(Vec::len), Some(1));
}
