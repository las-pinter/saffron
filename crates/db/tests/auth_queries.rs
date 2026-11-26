//! Queries behind sign-in: users, sessions, throttling, and the audit log.

use jiff::{SignedDuration, Timestamp};
use saffron_db::{
    audit, is_unique_violation, login_throttle, open_in_memory, sessions,
    users::{self, NewUser},
};
use sqlx::SqlitePool;

fn at(text: &str) -> Timestamp {
    text.parse().expect("valid test timestamp")
}

fn now() -> Timestamp {
    at("2026-11-25T12:00:00Z")
}

fn minutes(count: i64) -> SignedDuration {
    SignedDuration::from_mins(count)
}

async fn db() -> SqlitePool {
    open_in_memory().await.expect("in-memory database")
}

fn new_user(username: &str) -> NewUser<'_> {
    NewUser {
        username,
        display_name: username,
        is_admin: false,
        password_hash: Some("$argon2id$placeholder"),
    }
}

async fn create(pool: &SqlitePool, username: &str) -> i64 {
    users::create(pool, new_user(username), now())
        .await
        .expect("create user")
}

#[tokio::test]
async fn create_first_succeeds_on_an_empty_table() {
    let pool = db().await;

    let id = users::create_first(&pool, new_user("admin"), now())
        .await
        .expect("query");

    assert!(id.is_some());
}

#[tokio::test]
async fn create_first_refuses_once_a_user_exists() {
    let pool = db().await;
    create(&pool, "existing").await;

    let id = users::create_first(&pool, new_user("intruder"), now())
        .await
        .expect("query");

    assert_eq!(id, None);
    assert_eq!(users::count(&pool).await.expect("count"), 1);
}

#[tokio::test]
async fn a_taken_username_is_a_unique_violation() {
    let pool = db().await;
    create(&pool, "Las").await;

    let error = users::create(&pool, new_user("LAS"), now())
        .await
        .expect_err("duplicate username");

    assert!(is_unique_violation(&error), "error: {error:?}");
}

#[tokio::test]
async fn by_username_ignores_case() {
    let pool = db().await;
    let id = create(&pool, "Las").await;

    let user = users::by_username(&pool, "las").await.expect("query");

    assert_eq!(user.map(|user| user.id), Some(id));
}

#[tokio::test]
async fn a_disabled_user_is_not_active_until_enabled() {
    let pool = db().await;
    let id = create(&pool, "leaving").await;

    users::set_disabled(&pool, id, Some(now()))
        .await
        .expect("disable");
    let disabled = users::by_id(&pool, id).await.expect("query").expect("user");
    users::set_disabled(&pool, id, None).await.expect("enable");
    let enabled = users::by_id(&pool, id).await.expect("query").expect("user");

    assert_eq!((disabled.is_active(), enabled.is_active()), (false, true));
}

#[tokio::test]
async fn set_password_hash_reports_a_missing_user() {
    let pool = db().await;

    let found = users::set_password_hash(&pool, 404, "$argon2id$x")
        .await
        .expect("query");

    assert!(!found);
}

#[tokio::test]
async fn a_session_round_trips() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    let expires = now() + SignedDuration::from_hours(720);
    sessions::insert(&pool, b"hash", user, now(), expires)
        .await
        .expect("insert");

    let session = sessions::find(&pool, b"hash").await.expect("query");

    assert_eq!(
        session,
        Some(sessions::Session {
            user_id: user,
            created_at: now(),
            last_seen_at: now(),
            expires_at: expires,
        })
    );
}

#[tokio::test]
async fn touch_moves_last_seen() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    sessions::insert(&pool, b"hash", user, now(), now() + minutes(60))
        .await
        .expect("insert");

    sessions::touch(&pool, b"hash", now() + minutes(5))
        .await
        .expect("touch");

    let session = sessions::find(&pool, b"hash")
        .await
        .expect("query")
        .expect("row");
    assert_eq!(session.last_seen_at, now() + minutes(5));
}

#[tokio::test]
async fn delete_for_user_keeps_the_named_session() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    for hash in [b"keep", b"drop"] {
        sessions::insert(&pool, hash, user, now(), now() + minutes(60))
            .await
            .expect("insert");
    }

    let deleted = sessions::delete_for_user(&pool, user, Some(b"keep"))
        .await
        .expect("delete");

    assert_eq!(deleted, 1);
    assert!(
        sessions::find(&pool, b"keep")
            .await
            .expect("query")
            .is_some()
    );
}

#[tokio::test]
async fn delete_for_user_without_keep_ends_every_session() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    for hash in [b"one", b"two"] {
        sessions::insert(&pool, hash, user, now(), now() + minutes(60))
            .await
            .expect("insert");
    }

    let deleted = sessions::delete_for_user(&pool, user, None)
        .await
        .expect("delete");

    assert_eq!(deleted, 2);
}

#[tokio::test]
async fn username_failures_count_only_inside_the_window_ignoring_case() {
    let pool = db().await;
    for (name, minutes_ago) in [("las", 20), ("LAS", 10), ("Las", 1), ("other", 1)] {
        login_throttle::record_failure(&pool, name, "peer", now() - minutes(minutes_ago))
            .await
            .expect("record");
    }

    let count = login_throttle::failures_for_username(&pool, "las", now() - minutes(15))
        .await
        .expect("count");

    assert_eq!(count, 2);
}

#[tokio::test]
async fn source_failures_report_count_and_oldest() {
    let pool = db().await;
    for minutes_ago in [30, 12, 3] {
        login_throttle::record_failure(&pool, "any", "10.0.0.1", now() - minutes(minutes_ago))
            .await
            .expect("record");
    }

    let window = login_throttle::failures_for_source(&pool, "10.0.0.1", now() - minutes(15))
        .await
        .expect("count");

    assert_eq!(window, (2, Some(now() - minutes(12))));
}

#[tokio::test]
async fn clear_failures_forgets_one_username() {
    let pool = db().await;
    for name in ["las", "other"] {
        login_throttle::record_failure(&pool, name, "peer", now())
            .await
            .expect("record");
    }

    login_throttle::clear_failures(&pool, "LAS")
        .await
        .expect("clear");

    let since = now() - minutes(1);
    let remaining = (
        login_throttle::failures_for_username(&pool, "las", since)
            .await
            .expect("count"),
        login_throttle::failures_for_username(&pool, "other", since)
            .await
            .expect("count"),
    );
    assert_eq!(remaining, (0, 1));
}

#[tokio::test]
async fn prune_failures_deletes_only_older_rows() {
    let pool = db().await;
    for minutes_ago in [120, 5] {
        login_throttle::record_failure(&pool, "las", "peer", now() - minutes(minutes_ago))
            .await
            .expect("record");
    }

    login_throttle::prune_failures(&pool, now() - minutes(60))
        .await
        .expect("prune");

    let counts = (
        login_throttle::failures_for_username(&pool, "las", now() - minutes(1000))
            .await
            .expect("count all"),
        login_throttle::failures_for_username(&pool, "las", now() - minutes(10))
            .await
            .expect("count recent"),
    );
    assert_eq!(counts, (1, 1));
}

#[tokio::test]
async fn a_lock_is_replaced_read_and_removed() {
    let pool = db().await;
    login_throttle::lock(&pool, "las", now())
        .await
        .expect("lock");
    login_throttle::lock(&pool, "LAS", now() + minutes(15))
        .await
        .expect("relock");

    let until = login_throttle::locked_until(&pool, "Las")
        .await
        .expect("query");
    login_throttle::unlock(&pool, "las").await.expect("unlock");
    let after = login_throttle::locked_until(&pool, "las")
        .await
        .expect("query");

    assert_eq!((until, after), (Some(now() + minutes(15)), None));
}

#[tokio::test]
async fn prune_locks_deletes_only_ended_locks() {
    let pool = db().await;
    login_throttle::lock(&pool, "ended", now() - minutes(1))
        .await
        .expect("lock");
    login_throttle::lock(&pool, "active", now() + minutes(1))
        .await
        .expect("lock");

    login_throttle::prune_locks(&pool, now())
        .await
        .expect("prune");

    let locks = (
        login_throttle::locked_until(&pool, "ended")
            .await
            .expect("query"),
        login_throttle::locked_until(&pool, "active")
            .await
            .expect("query"),
    );
    assert_eq!(locks, (None, Some(now() + minutes(1))));
}

#[tokio::test]
async fn delete_ended_removes_expired_and_idle_sessions() {
    let pool = db().await;
    let user = create(&pool, "owner").await;
    // (hash, started, absolute end)
    for (hash, started, ends) in [
        (b"expired", now() - minutes(60), now() - minutes(1)),
        (b"idle_00", now() - minutes(60), now() + minutes(60)),
        (b"live_00", now(), now() + minutes(60)),
    ] {
        sessions::insert(&pool, hash, user, started, ends)
            .await
            .expect("insert");
    }

    let deleted = sessions::delete_ended(&pool, now(), now() - minutes(30))
        .await
        .expect("delete");

    assert_eq!(deleted, 2);
    assert!(
        sessions::find(&pool, b"live_00")
            .await
            .expect("query")
            .is_some()
    );
}

#[tokio::test]
async fn audit_entries_are_stored() {
    let pool = db().await;
    let user = create(&pool, "owner").await;

    audit::record(
        &pool,
        now(),
        audit::Entry {
            user_id: Some(user),
            action: "auth.login",
            target: Some("owner"),
            detail: None,
        },
    )
    .await
    .expect("record");

    let row: (String, Option<i64>, String) =
        sqlx::query_as("SELECT at, user_id, action FROM audit_log")
            .fetch_one(&pool)
            .await
            .expect("read");
    assert_eq!(
        row,
        (
            "2026-11-25T12:00:00Z".to_owned(),
            Some(user),
            "auth.login".to_owned()
        )
    );
}
