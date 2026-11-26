//! User accounts.

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::time;

/// One user row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct User {
    /// Row id.
    pub id: i64,
    /// Sign-in name, unique ignoring case.
    pub username: String,
    /// Name shown in the interface.
    pub display_name: String,
    /// Whether the user manages other users.
    pub is_admin: bool,
    /// Argon2id PHC string; `None` for a passkey-only user.
    pub password_hash: Option<String>,
    /// Encrypted TOTP secret; `None` when TOTP is off.
    pub totp_secret: Option<String>,
    /// Encrypted TOTP secret awaiting its first code during enrolment.
    pub totp_pending: Option<String>,
    /// Last accepted TOTP time step.
    pub totp_last_step: Option<i64>,
    /// When the account was disabled; `None` while it is active.
    pub disabled_at: Option<String>,
}

impl User {
    /// Whether the account may sign in.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.disabled_at.is_none()
    }
}

/// The fields of a new user.
#[derive(Debug, Clone, Copy)]
pub struct NewUser<'a> {
    /// Sign-in name.
    pub username: &'a str,
    /// Name shown in the interface.
    pub display_name: &'a str,
    /// Whether the user manages other users.
    pub is_admin: bool,
    /// Argon2id PHC string.
    pub password_hash: Option<&'a str>,
}

/// A static `SELECT` of every [`User`] column, followed by `$tail`.
macro_rules! select_users {
    ($tail:literal) => {
        concat!(
            "SELECT id, username, display_name, is_admin, password_hash, totp_secret,
                    totp_pending, totp_last_step, disabled_at
             FROM users ",
            $tail
        )
    };
}

/// Number of users, disabled ones included.
pub async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await
}

/// Any one sealed TOTP secret, enabled or pending, with its user's id; `None` when
/// nobody holds one.
pub async fn a_sealed_secret(pool: &SqlitePool) -> Result<Option<(i64, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, coalesce(totp_secret, totp_pending) FROM users
         WHERE totp_secret IS NOT NULL OR totp_pending IS NOT NULL
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

/// Creates a user and returns its id.
///
/// A taken username fails with a unique-constraint error; see
/// [`crate::is_unique_violation`].
pub async fn create(
    pool: &SqlitePool,
    user: NewUser<'_>,
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (username, display_name, is_admin, password_hash, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user.username)
    .bind(user.display_name)
    .bind(user.is_admin)
    .bind(user.password_hash)
    .bind(time::to_text(now))
    .fetch_one(pool)
    .await
}

/// Creates `user` only while the table is empty, in one statement, so two
/// concurrent first-run requests cannot both succeed. Returns `None` when a user
/// already exists.
pub async fn create_first(
    pool: &SqlitePool,
    user: NewUser<'_>,
    now: Timestamp,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (username, display_name, is_admin, password_hash, created_at)
         SELECT ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users) RETURNING id",
    )
    .bind(user.username)
    .bind(user.display_name)
    .bind(user.is_admin)
    .bind(user.password_hash)
    .bind(time::to_text(now))
    .fetch_optional(pool)
    .await
}

/// The user with `id`.
pub async fn by_id(pool: &SqlitePool, id: i64) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as(select_users!("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// The user named `username`, ignoring case.
pub async fn by_username(pool: &SqlitePool, username: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as(select_users!("WHERE username = ?"))
        .bind(username)
        .fetch_optional(pool)
        .await
}

/// Every user, ordered by username.
pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, sqlx::Error> {
    sqlx::query_as(select_users!("ORDER BY username"))
        .fetch_all(pool)
        .await
}

/// Replaces the password hash. Returns whether the user exists.
pub async fn set_password_hash(
    pool: &SqlitePool,
    id: i64,
    password_hash: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(password_hash)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Disables the user at `at`, or enables it with `None`. Returns whether the user
/// exists.
pub async fn set_disabled(
    pool: &SqlitePool,
    id: i64,
    at: Option<Timestamp>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE users SET disabled_at = ? WHERE id = ?")
        .bind(at.map(time::to_text))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Stores, or with `None` discards, the secret offered during TOTP enrolment.
/// Returns whether the user exists.
pub async fn set_totp_pending(
    pool: &SqlitePool,
    id: i64,
    sealed: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE users SET totp_pending = ? WHERE id = ?")
        .bind(sealed)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Turns TOTP on with `sealed` as the secret, recording `step` as used.
///
/// Only while `sealed` is still the pending secret and TOTP is off, so a parallel
/// `setup` that replaced it is never enabled unseen; `false` otherwise.
pub async fn enable_totp(
    pool: &SqlitePool,
    id: i64,
    sealed: &str,
    step: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE users SET totp_secret = ?, totp_pending = NULL, totp_last_step = ?
         WHERE id = ? AND totp_pending = ? AND totp_secret IS NULL",
    )
    .bind(sealed)
    .bind(step)
    .bind(id)
    .bind(sealed)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Turns TOTP off and forgets any pending enrolment.
pub async fn disable_totp(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_pending = NULL, totp_last_step = NULL
         WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Records `step` as used, in one statement; returns `false` when that step or a
/// later one was already used, so a code is never accepted twice.
pub async fn claim_totp_step(pool: &SqlitePool, id: i64, step: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE users SET totp_last_step = ?
         WHERE id = ? AND (totp_last_step IS NULL OR totp_last_step < ?)",
    )
    .bind(step)
    .bind(id)
    .bind(step)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}
