//! Queries behind two-factor sign-in: enrolment, the sealed secret, and replay
//! protection.

use saffron_db::{
    open_in_memory,
    users::{self, NewUser},
};
use sqlx::SqlitePool;

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
    let now = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    users::create(pool, user, now).await.expect("create user")
}

/// Enrols `id` with the secret "sealed", as setup and confirm do.
async fn enrol(pool: &SqlitePool, id: i64, step: i64) {
    users::set_totp_pending(pool, id, Some("sealed"))
        .await
        .expect("pending");
    let enabled = users::enable_totp(pool, id, "sealed", step)
        .await
        .expect("enable");
    assert!(enabled, "enrolment");
}

#[tokio::test]
async fn enabling_totp_moves_the_pending_secret_into_place() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    users::set_totp_pending(&pool, id, Some("sealed"))
        .await
        .expect("pending");

    let enabled = users::enable_totp(&pool, id, "sealed", 42)
        .await
        .expect("enable");

    assert!(enabled);
    let user = users::by_id(&pool, id).await.expect("query").expect("user");
    assert_eq!(
        (
            user.totp_secret.as_deref(),
            user.totp_pending,
            user.totp_last_step
        ),
        (Some("sealed"), None, Some(42))
    );
}

#[tokio::test]
async fn a_replaced_pending_secret_is_not_enabled() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    users::set_totp_pending(&pool, id, Some("newer"))
        .await
        .expect("pending");

    let enabled = users::enable_totp(&pool, id, "older", 42)
        .await
        .expect("enable");

    assert!(!enabled);
    let user = users::by_id(&pool, id).await.expect("query").expect("user");
    assert_eq!(
        (user.totp_secret, user.totp_pending.as_deref()),
        (None, Some("newer"))
    );
}

#[tokio::test]
async fn an_enabled_secret_is_not_replaced() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    enrol(&pool, id, 1).await;
    sqlx::query("UPDATE users SET totp_pending = 'other' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("pending beside an enabled secret");

    let enabled = users::enable_totp(&pool, id, "other", 2)
        .await
        .expect("enable");

    assert!(!enabled);
    let user = users::by_id(&pool, id).await.expect("query").expect("user");
    assert_eq!(user.totp_secret.as_deref(), Some("sealed"));
}

#[tokio::test]
async fn no_sealed_secret_is_found_when_nobody_enrolled() {
    let pool = db().await;
    create(&pool, "owner").await;

    let found = users::a_sealed_secret(&pool).await.expect("query");

    assert_eq!(found, None);
}

#[tokio::test]
async fn a_pending_secret_counts_as_sealed() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    users::set_totp_pending(&pool, id, Some("sealed"))
        .await
        .expect("pending");

    let found = users::a_sealed_secret(&pool).await.expect("query");

    assert_eq!(found, Some((id, "sealed".to_owned())));
}

#[tokio::test]
async fn an_enabled_secret_counts_as_sealed() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    enrol(&pool, id, 1).await;

    let found = users::a_sealed_secret(&pool).await.expect("query");

    assert_eq!(found, Some((id, "sealed".to_owned())));
}

#[tokio::test]
async fn disabling_totp_clears_every_totp_field() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    enrol(&pool, id, 42).await;
    users::set_totp_pending(&pool, id, Some("next"))
        .await
        .expect("pending");

    users::disable_totp(&pool, id).await.expect("disable");

    let user = users::by_id(&pool, id).await.expect("query").expect("user");
    assert_eq!(
        (user.totp_secret, user.totp_pending, user.totp_last_step),
        (None, None, None)
    );
}

#[tokio::test]
async fn a_totp_step_is_claimed_only_once() {
    let pool = db().await;
    let id = create(&pool, "owner").await;
    enrol(&pool, id, 10).await;

    let claims = (
        users::claim_totp_step(&pool, id, 11).await.expect("claim"),
        users::claim_totp_step(&pool, id, 11)
            .await
            .expect("claim again"),
        users::claim_totp_step(&pool, id, 10)
            .await
            .expect("claim older"),
    );

    assert_eq!(claims, (true, false, false));
}
