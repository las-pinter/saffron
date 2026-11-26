//! Sign-in sessions, keyed by the SHA-256 of the session token.

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::time::{self, InvalidTimestamp};

/// One session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Owner of the session.
    pub user_id: i64,
    /// When the session started.
    pub created_at: Timestamp,
    /// Last request that touched the session.
    pub last_seen_at: Timestamp,
    /// Absolute end of the session.
    pub expires_at: Timestamp,
}

/// Session lookup failures.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The database rejected the query.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// A stored timestamp is corrupt.
    #[error(transparent)]
    Timestamp(#[from] InvalidTimestamp),
}

/// Stores a new session.
pub async fn insert(
    pool: &SqlitePool,
    token_hash: &[u8],
    user_id: i64,
    now: Timestamp,
    expires_at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, created_at, last_seen_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(time::to_text(now))
    .bind(time::to_text(now))
    .bind(time::to_text(expires_at))
    .execute(pool)
    .await?;
    Ok(())
}

/// The session with `token_hash`, expired or not.
pub async fn find(pool: &SqlitePool, token_hash: &[u8]) -> Result<Option<Session>, SessionError> {
    let row: Option<(i64, String, String, String)> = sqlx::query_as(
        "SELECT user_id, created_at, last_seen_at, expires_at FROM sessions WHERE token_hash = ?",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    let Some((user_id, created_at, last_seen_at, expires_at)) = row else {
        return Ok(None);
    };
    Ok(Some(Session {
        user_id,
        created_at: time::from_text(&created_at)?,
        last_seen_at: time::from_text(&last_seen_at)?,
        expires_at: time::from_text(&expires_at)?,
    }))
}

/// Records activity on a session.
pub async fn touch(
    pool: &SqlitePool,
    token_hash: &[u8],
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
        .bind(time::to_text(now))
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Ends one session.
pub async fn delete(pool: &SqlitePool, token_hash: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes sessions past their absolute end at `now`, or last seen before
/// `idle_before`.
pub async fn delete_ended(
    pool: &SqlitePool,
    now: Timestamp,
    idle_before: Timestamp,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ? OR last_seen_at <= ?")
        .bind(time::to_text(now))
        .bind(time::to_text(idle_before))
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Ends every session of a user except `keep`, when given.
pub async fn delete_for_user(
    pool: &SqlitePool,
    user_id: i64,
    keep: Option<&[u8]>,
) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND (? IS NULL OR token_hash <> ?)")
            .bind(user_id)
            .bind(keep)
            .bind(keep)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}
