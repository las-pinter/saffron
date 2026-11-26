//! Discoverable passkey sign-in over HTTP, with a software authenticator, and the
//! ways a sign-in is refused.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    LOCAL_ORIGIN, TestApp, USER_PASSWORD,
    authenticator::SoftAuthenticator,
    passkey_steps::{admin_with_passkey, error, list, register, sign_in, turn_on_totp},
};
use jiff::SignedDuration;
use serde_json::json;

#[tokio::test]
async fn a_registered_passkey_signs_in() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;

    let reply = sign_in(&app, &mut authenticator).await;

    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
    assert_eq!(reply.json()["user"]["username"], json!("admin"));
    assert!(reply.session_cookie().is_some());
}

#[tokio::test]
async fn a_passkey_sign_in_skips_totp() {
    let app = TestApp::new().await;
    let (cookie, mut authenticator) = admin_with_passkey(&app).await;
    turn_on_totp(&app, &cookie).await;

    let reply = sign_in(&app, &mut authenticator).await;

    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
}

#[tokio::test]
async fn an_expired_sign_in_ceremony_is_refused() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;
    let started = app
        .post("/api/auth/passkey/login/start")
        .send()
        .await
        .json();
    let credential = authenticator.sign_in(&started["options"]);
    app.advance(SignedDuration::from_mins(5));

    let reply = app
        .post("/api/auth/passkey/login/finish")
        .json(json!({ "ceremony_id": started["ceremony_id"], "credential": credential }))
        .send()
        .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("ceremony_expired"))
    );
}

#[tokio::test]
async fn a_sign_in_records_when_the_passkey_was_used() {
    let app = TestApp::new().await;
    let (cookie, mut authenticator) = admin_with_passkey(&app).await;
    app.advance(SignedDuration::from_mins(1));

    sign_in(&app, &mut authenticator).await;

    let used = &list(&app, &cookie).await[0]["last_used_at"];
    assert_eq!(
        *used,
        json!(saffron_db::time::to_text(app.state.clock.now()))
    );
}

#[tokio::test]
async fn a_deleted_passkey_cannot_sign_in() {
    let app = TestApp::new().await;
    let (cookie, mut authenticator) = admin_with_passkey(&app).await;
    let id = list(&app, &cookie).await[0]["id"].clone();
    let deleted = app
        .call(Method::DELETE, &format!("/api/auth/passkeys/{id}"))
        .cookie(&cookie)
        .send()
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let reply = sign_in(&app, &mut authenticator).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("passkey_failed"))
    );
}

#[tokio::test]
async fn a_disabled_user_cannot_sign_in_with_a_passkey() {
    let app = TestApp::new().await;
    let admin = app.setup_admin().await;
    let member_id = app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");
    let mut authenticator = SoftAuthenticator::new(LOCAL_ORIGIN);
    register(&app, &member, USER_PASSWORD, &mut authenticator).await;
    app.post(&format!("/api/users/{member_id}/disable"))
        .cookie(&admin)
        .send()
        .await;

    let reply = sign_in(&app, &mut authenticator).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("passkey_failed"))
    );
}

#[tokio::test]
async fn a_counter_that_goes_back_is_refused() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;
    sign_in(&app, &mut authenticator).await;
    authenticator.counter -= 1;

    let reply = sign_in(&app, &mut authenticator).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("passkey_failed"))
    );
}

#[tokio::test]
async fn a_credential_without_a_user_handle_is_refused() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;
    authenticator.user_handle = None;

    let reply = sign_in(&app, &mut authenticator).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("passkey_failed"))
    );
}

#[tokio::test]
async fn a_passkey_cannot_sign_in_as_another_user() {
    let app = TestApp::new().await;
    let (admin, admin_key) = admin_with_passkey(&app).await;
    app.create_user(&admin, "member").await;
    let member = app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");
    let mut member_key = SoftAuthenticator::new(LOCAL_ORIGIN);
    register(&app, &member, USER_PASSWORD, &mut member_key).await;
    member_key.user_handle = admin_key.user_handle;

    let reply = sign_in(&app, &mut member_key).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("passkey_failed"))
    );
}

#[tokio::test]
async fn a_signature_from_another_key_is_refused() {
    let app = TestApp::new().await;
    let (_, authenticator) = admin_with_passkey(&app).await;
    let mut impostor = SoftAuthenticator::new(LOCAL_ORIGIN);
    impostor.credential_id = authenticator.credential_id;
    impostor.user_handle = authenticator.user_handle;
    impostor.counter = authenticator.counter;

    let reply = sign_in(&app, &mut impostor).await;

    assert_eq!(
        error(&reply),
        (StatusCode::UNAUTHORIZED, json!("passkey_failed"))
    );
}

#[tokio::test]
async fn a_passkey_sign_in_is_audited() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;

    sign_in(&app, &mut authenticator).await;

    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
        .fetch_all(&app.state.pool)
        .await
        .expect("audit");
    assert!(
        actions.ends_with(&["auth.passkey_login".to_owned()]),
        "{actions:?}"
    );
}

#[tokio::test]
async fn an_ip_address_origin_has_no_passkeys() {
    let app = TestApp::with_origin("https://100.64.0.1").await;

    let reply = app.post("/api/auth/passkey/login/start").send().await;

    assert_eq!(
        error(&reply),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!("passkeys_unavailable")
        )
    );
}

/// A sign-in answered by a passkey the server does not know.
async fn unknown_passkey_sign_in(app: &TestApp) -> common::Reply {
    let mut stranger = SoftAuthenticator::new(LOCAL_ORIGIN);
    stranger.user_handle = Some(b"0123456789abcdef".to_vec());
    sign_in(app, &mut stranger).await
}

#[tokio::test]
async fn refused_passkeys_count_toward_the_client_brake() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for _ in 0..20 {
        unknown_passkey_sign_in(&app).await;
    }

    let reply = app.post("/api/auth/passkey/login/start").send().await;

    assert_eq!(
        error(&reply),
        (StatusCode::TOO_MANY_REQUESTS, json!("too_many_attempts"))
    );
}

#[tokio::test]
async fn a_braked_client_cannot_finish_a_passkey_sign_in() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;
    let started = app
        .post("/api/auth/passkey/login/start")
        .send()
        .await
        .json();
    for _ in 0..20 {
        unknown_passkey_sign_in(&app).await;
    }
    let credential = authenticator.sign_in(&started["options"]);

    let reply = app
        .post("/api/auth/passkey/login/finish")
        .json(json!({ "ceremony_id": started["ceremony_id"], "credential": credential }))
        .send()
        .await;

    assert_eq!(
        error(&reply),
        (StatusCode::TOO_MANY_REQUESTS, json!("too_many_attempts"))
    );
}

#[tokio::test]
async fn refused_passkeys_never_lock_an_account() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for _ in 0..6 {
        unknown_passkey_sign_in(&app).await;
    }

    let reply = app.login("admin", common::ADMIN_PASSWORD).await;

    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
}

#[tokio::test]
async fn an_unknown_passkey_writes_no_audit_row() {
    let app = TestApp::new().await;
    app.setup_admin().await;

    unknown_passkey_sign_in(&app).await;

    let failed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'auth.passkey_failed'")
            .fetch_one(&app.state.pool)
            .await
            .expect("audit");
    assert_eq!(failed, 0);
}

#[tokio::test]
async fn a_refused_known_passkey_is_audited() {
    let app = TestApp::new().await;
    let (_, mut authenticator) = admin_with_passkey(&app).await;
    sign_in(&app, &mut authenticator).await;
    authenticator.counter -= 1;

    sign_in(&app, &mut authenticator).await;

    let failed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'auth.passkey_failed'")
            .fetch_one(&app.state.pool)
            .await
            .expect("audit");
    assert_eq!(failed, 1);
}
