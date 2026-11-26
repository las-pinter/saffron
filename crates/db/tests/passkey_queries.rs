//! Queries behind passkey sign-in: stored passkeys and `WebAuthn` user handles.

use jiff::Timestamp;
use saffron_db::{
    is_unique_violation, open_in_memory,
    passkeys::{self, NewPasskey},
    users::{self, NewUser},
};
use sqlx::SqlitePool;

fn at(text: &str) -> Timestamp {
    text.parse().expect("valid test timestamp")
}

async fn db() -> SqlitePool {
    open_in_memory().await.expect("in-memory database")
}

async fn create(pool: &SqlitePool, username: &str) -> i64 {
    let user = NewUser {
        username,
        display_name: username,
        is_admin: false,
        password_hash: Some("$argon2id$placeholder"),
    };
    users::create(pool, user, at("2026-11-25T12:00:00Z"))
        .await
        .expect("create user")
}

fn new_passkey<'a>(user_id: i64, credential_id: &'a [u8], name: &'a str) -> NewPasskey<'a> {
    NewPasskey {
        user_id,
        credential_id,
        credential: r#"{"cred":1}"#,
        name,
    }
}

async fn add(pool: &SqlitePool, user_id: i64, credential_id: &[u8], name: &str) -> i64 {
    passkeys::insert(
        pool,
        new_passkey(user_id, credential_id, name),
        at("2026-11-25T12:00:00Z"),
    )
    .await
    .expect("insert passkey")
}

#[tokio::test]
async fn a_passkey_is_found_by_its_credential_id() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    let id = add(&pool, user, b"cred-1", "Laptop").await;

    let found = passkeys::by_credential_id(&pool, b"cred-1")
        .await
        .expect("query");

    assert_eq!(
        found,
        Some(passkeys::Passkey {
            id,
            user_id: user,
            credential_id: b"cred-1".to_vec(),
            credential: r#"{"cred":1}"#.to_owned(),
            name: "Laptop".to_owned(),
            created_at: "2026-11-25T12:00:00Z".to_owned(),
            last_used_at: None,
        })
    );
}

#[tokio::test]
async fn a_credential_id_is_registered_only_once() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    add(&pool, user, b"cred-1", "Laptop").await;

    let error = passkeys::insert(
        &pool,
        new_passkey(user, b"cred-1", "Again"),
        at("2026-11-25T12:00:00Z"),
    )
    .await
    .expect_err("duplicate");

    assert!(is_unique_violation(&error), "{error}");
}

#[tokio::test]
async fn a_user_lists_only_their_own_passkeys_in_order() {
    let pool = db().await;
    let owner = create(&pool, "owner").await;
    let other = create(&pool, "other").await;
    add(&pool, owner, b"cred-1", "First").await;
    add(&pool, other, b"cred-2", "Theirs").await;
    add(&pool, owner, b"cred-3", "Second").await;

    let listed = passkeys::for_user(&pool, owner).await.expect("query");

    let names: Vec<_> = listed.iter().map(|passkey| passkey.name.as_str()).collect();
    assert_eq!(names, ["First", "Second"]);
}

#[tokio::test]
async fn record_use_stores_the_new_credential_and_time() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    let id = add(&pool, user, b"cred-1", "Laptop").await;

    passkeys::record_use(&pool, id, r#"{"cred":2}"#, at("2026-11-26T08:00:00Z"))
        .await
        .expect("record use");

    let found = passkeys::by_credential_id(&pool, b"cred-1")
        .await
        .expect("query")
        .expect("passkey");
    assert_eq!(
        (found.credential.as_str(), found.last_used_at.as_deref()),
        (r#"{"cred":2}"#, Some("2026-11-26T08:00:00Z"))
    );
}

#[tokio::test]
async fn a_passkey_is_deleted_only_by_its_owner() {
    let pool = db().await;
    let owner = create(&pool, "owner").await;
    let other = create(&pool, "other").await;
    let id = add(&pool, owner, b"cred-1", "Laptop").await;

    let deleted = (
        passkeys::delete(&pool, other, id).await.expect("delete"),
        passkeys::delete(&pool, owner, id).await.expect("delete"),
    );

    assert_eq!(deleted, (false, true));
    assert_eq!(
        passkeys::by_credential_id(&pool, b"cred-1")
            .await
            .expect("query"),
        None
    );
}

#[tokio::test]
async fn the_first_webauthn_handle_is_kept() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    users::webauthn_id_or_set(&pool, user, b"handle-one")
        .await
        .expect("first");

    let handle = users::webauthn_id_or_set(&pool, user, b"handle-two")
        .await
        .expect("second");

    assert_eq!(handle, Some(b"handle-one".to_vec()));
}

#[tokio::test]
async fn a_missing_user_has_no_webauthn_handle() {
    let pool = db().await;

    let handle = users::webauthn_id_or_set(&pool, 404, b"handle")
        .await
        .expect("query");

    assert_eq!(handle, None);
}

#[tokio::test]
async fn two_users_cannot_share_a_webauthn_handle() {
    let pool = db().await;
    let owner = create(&pool, "owner").await;
    let other = create(&pool, "other").await;
    users::webauthn_id_or_set(&pool, owner, b"handle")
        .await
        .expect("owner");

    let error = users::webauthn_id_or_set(&pool, other, b"handle")
        .await
        .expect_err("shared handle");

    assert!(is_unique_violation(&error), "{error}");
}

#[tokio::test]
async fn a_user_is_found_by_webauthn_handle() {
    let pool = db().await;
    let owner = create(&pool, "owner").await;
    users::webauthn_id_or_set(&pool, owner, b"handle")
        .await
        .expect("set");

    let found = (
        users::by_webauthn_id(&pool, b"handle")
            .await
            .expect("query"),
        users::by_webauthn_id(&pool, b"unknown")
            .await
            .expect("query"),
    );

    assert_eq!(
        (found.0.map(|user| user.id), found.1.map(|user| user.id)),
        (Some(owner), None)
    );
}
