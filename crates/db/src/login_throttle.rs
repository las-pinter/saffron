//! Failed sign-in attempts and account locks.

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::time::{self, InvalidTimestamp};

/// Throttle lookup failures.
#[derive(Debug, thiserror::Error)]
pub enum ThrottleError {
    /// The database rejected the query.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// A stored timestamp is corrupt.
    #[error(transparent)]
    Timestamp(#[from] InvalidTimestamp),
}

/// Records one failed attempt.
pub async fn record_failure(
    pool: &SqlitePool,
    username: &str,
    source: &str,
    at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO login_failures (username, source, at) VALUES (?, ?, ?)")
        .bind(username)
        .bind(source)
        .bind(time::to_text(at))
        .execute(pool)
        .await?;
    Ok(())
}

/// Failures for `username` at or after `since`.
pub async fn failures_for_username(
    pool: &SqlitePool,
    username: &str,
    since: Timestamp,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM login_failures WHERE username = ? AND at >= ?")
        .bind(username)
        .bind(time::to_text(since))
        .fetch_one(pool)
        .await
}

/// Failures from `source` at or after `since`, and the oldest of them.
pub async fn failures_for_source(
    pool: &SqlitePool,
    source: &str,
    since: Timestamp,
) -> Result<(i64, Option<Timestamp>), ThrottleError> {
    let (count, oldest): (i64, Option<String>) =
        sqlx::query_as("SELECT count(*), min(at) FROM login_failures WHERE source = ? AND at >= ?")
            .bind(source)
            .bind(time::to_text(since))
            .fetch_one(pool)
            .await?;
    let oldest = oldest.as_deref().map(time::from_text).transpose()?;
    Ok((count, oldest))
}

/// Forgets the failures of `username`, after a successful sign-in.
pub async fn clear_failures(pool: &SqlitePool, username: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_failures WHERE username = ?")
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes failures older than `before`.
pub async fn prune_failures(pool: &SqlitePool, before: Timestamp) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_failures WHERE at < ?")
        .bind(time::to_text(before))
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes locks that ended before `now`.
pub async fn prune_locks(pool: &SqlitePool, now: Timestamp) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_locks WHERE locked_until < ?")
        .bind(time::to_text(now))
        .execute(pool)
        .await?;
    Ok(())
}

/// Locks `username` until `until`.
pub async fn lock(pool: &SqlitePool, username: &str, until: Timestamp) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO login_locks (username, locked_until) VALUES (?, ?)
         ON CONFLICT (username) DO UPDATE SET locked_until = excluded.locked_until",
    )
    .bind(username)
    .bind(time::to_text(until))
    .execute(pool)
    .await?;
    Ok(())
}

/// When the lock on `username` ends, if one is stored (it may lie in the past).
pub async fn locked_until(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<Timestamp>, ThrottleError> {
    let until: Option<String> =
        sqlx::query_scalar("SELECT locked_until FROM login_locks WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await?;
    Ok(until.as_deref().map(time::from_text).transpose()?)
}

/// Removes the lock on `username`.
pub async fn unlock(pool: &SqlitePool, username: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_locks WHERE username = ?")
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}
