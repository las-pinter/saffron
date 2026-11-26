//! Session lifetime rules.
//!
//! A session ends 30 days after sign-in, or after 12 hours without a request,
//! whichever comes first, and at once when its user is disabled.

use jiff::{SignedDuration, Timestamp};
use saffron_db::{
    sessions,
    users::{self, User},
};
use sqlx::SqlitePool;

use super::token::{RandomError, TokenHash, hash_token, random_token};
use crate::error::ApiError;

/// Absolute session lifetime.
pub const ABSOLUTE_LIFETIME: SignedDuration = SignedDuration::from_hours(30 * 24);
/// Idle timeout.
pub const IDLE_TIMEOUT: SignedDuration = SignedDuration::from_hours(12);
/// Last-seen is rewritten at most this often, to spare the database a write per
/// request.
pub const TOUCH_AFTER: SignedDuration = SignedDuration::from_mins(5);

/// A live session and its user.
#[derive(Debug, Clone)]
pub struct LiveSession {
    /// The signed-in user.
    pub user: User,
    /// Hash of the session token.
    pub token_hash: TokenHash,
}

/// Starts a session for `user_id` and returns the cookie token.
pub async fn start(pool: &SqlitePool, user_id: i64, now: Timestamp) -> Result<String, ApiError> {
    // Sessions are otherwise deleted only when their token comes back; sweep the
    // abandoned ones whenever a new one starts.
    sessions::delete_ended(pool, now, now - IDLE_TIMEOUT).await?;
    let token = random_token().map_err(|error: RandomError| ApiError::internal(error))?;
    sessions::insert(
        pool,
        &hash_token(&token),
        user_id,
        now,
        now + ABSOLUTE_LIFETIME,
    )
    .await?;
    Ok(token)
}

/// The live session for `token`, or `None`. A session that ended is deleted.
pub async fn resolve(
    pool: &SqlitePool,
    token: &str,
    now: Timestamp,
) -> Result<Option<LiveSession>, ApiError> {
    let token_hash = hash_token(token);
    let Some(session) = sessions::find(pool, &token_hash).await? else {
        return Ok(None);
    };
    let ended = now >= session.expires_at || now >= session.last_seen_at + IDLE_TIMEOUT;
    let user = if ended {
        None
    } else {
        users::by_id(pool, session.user_id)
            .await?
            .filter(User::is_active)
    };
    let Some(user) = user else {
        sessions::delete(pool, &token_hash).await?;
        return Ok(None);
    };
    if now - session.last_seen_at >= TOUCH_AFTER {
        sessions::touch(pool, &token_hash, now).await?;
    }
    Ok(Some(LiveSession { user, token_hash }))
}

/// Seconds the cookie should live: the absolute session lifetime.
#[must_use]
pub const fn cookie_max_age_secs() -> i64 {
    ABSOLUTE_LIFETIME.as_secs()
}
