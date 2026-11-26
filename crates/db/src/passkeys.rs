//! Registered passkeys.

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::time;

/// One passkey row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Passkey {
    /// Row id.
    pub id: i64,
    /// Owner of the passkey.
    pub user_id: i64,
    /// `WebAuthn` credential id.
    pub credential_id: Vec<u8>,
    /// Serialized `WebAuthn` credential (public key and counter).
    pub credential: String,
    /// Name the user gave the passkey.
    pub name: String,
    /// When the passkey was registered, as RFC 3339 text.
    pub created_at: String,
    /// When the passkey last signed in, as RFC 3339 text.
    pub last_used_at: Option<String>,
}

/// A static `SELECT` of every [`Passkey`] column, followed by `$tail`.
macro_rules! select_passkeys {
    ($tail:literal) => {
        concat!(
            "SELECT id, user_id, credential_id, credential, name, created_at, last_used_at
             FROM passkeys ",
            $tail
        )
    };
}

/// The fields of a new passkey.
#[derive(Debug, Clone, Copy)]
pub struct NewPasskey<'a> {
    /// Owner of the passkey.
    pub user_id: i64,
    /// `WebAuthn` credential id.
    pub credential_id: &'a [u8],
    /// Serialized `WebAuthn` credential.
    pub credential: &'a str,
    /// Name the user gave the passkey.
    pub name: &'a str,
}

/// Stores a passkey and returns its id. A credential id already registered fails
/// with a unique-constraint error; see [`crate::is_unique_violation`].
pub async fn insert(
    pool: &SqlitePool,
    passkey: NewPasskey<'_>,
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO passkeys (user_id, credential_id, credential, name, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(passkey.user_id)
    .bind(passkey.credential_id)
    .bind(passkey.credential)
    .bind(passkey.name)
    .bind(time::to_text(now))
    .fetch_one(pool)
    .await
}

/// The passkeys of `user_id`, oldest first.
pub async fn for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<Passkey>, sqlx::Error> {
    sqlx::query_as(select_passkeys!("WHERE user_id = ? ORDER BY id"))
        .bind(user_id)
        .fetch_all(pool)
        .await
}

/// The passkey with `credential_id`.
pub async fn by_credential_id(
    pool: &SqlitePool,
    credential_id: &[u8],
) -> Result<Option<Passkey>, sqlx::Error> {
    sqlx::query_as(select_passkeys!("WHERE credential_id = ?"))
        .bind(credential_id)
        .fetch_optional(pool)
        .await
}

/// Stores the credential as updated by a sign-in (its counter) and the time of use.
pub async fn record_use(
    pool: &SqlitePool,
    id: i64,
    credential: &str,
    now: Timestamp,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE passkeys SET credential = ?, last_used_at = ? WHERE id = ?")
        .bind(credential)
        .bind(time::to_text(now))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Removes passkey `id` when it belongs to `user_id`.
pub async fn delete(pool: &SqlitePool, user_id: i64, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM passkeys WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}
