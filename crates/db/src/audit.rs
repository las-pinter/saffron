//! The audit log: who did what, and when.

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::time;

/// One audit entry.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// Acting user, when known.
    pub user_id: Option<i64>,
    /// Dotted action name, such as `auth.login`.
    pub action: &'a str,
    /// What the action applied to, such as a username.
    pub target: Option<&'a str>,
    /// Extra JSON detail.
    pub detail: Option<&'a str>,
}

/// Appends an entry.
pub async fn record(pool: &SqlitePool, at: Timestamp, entry: Entry<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_log (at, user_id, action, target, detail) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(time::to_text(at))
    .bind(entry.user_id)
    .bind(entry.action)
    .bind(entry.target)
    .bind(entry.detail)
    .execute(pool)
    .await?;
    Ok(())
}
