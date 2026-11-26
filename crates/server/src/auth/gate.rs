//! The sign-in turn: one password check at a time, behind the throttle.

use jiff::SignedDuration;
use saffron_db::users::User;
use tokio::sync::{MutexGuard, SemaphorePermit};

use super::{
    password,
    throttle::{self, Verdict},
    totp,
};
use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
    users::MAX_USERNAME_CHARS,
};

/// The submitted username as the throttle records it: trimmed, and cut to one
/// character more than any valid username, so a huge value cannot bloat the table.
#[must_use]
pub fn throttle_key(raw: &str) -> String {
    raw.trim().chars().take(MAX_USERNAME_CHARS + 1).collect()
}

/// Held while one password check and its outcome are recorded.
#[expect(missing_debug_implementations, reason = "a guard with nothing to show")]
pub struct SignInTurn<'a> {
    _slot: SemaphorePermit<'a>,
    _gate: MutexGuard<'a, ()>,
}

/// Waits for this request's turn to check a password, so parallel guesses cannot
/// overshoot the lockout and only one Argon2 verification runs at a time.
///
/// A throttled client is refused before it joins the queue, and the queue is
/// bounded, so a flood cannot hold up everyone else's sign-in for long.
pub async fn sign_in_turn<'a>(
    state: &'a AppState,
    username: &str,
    source: &str,
) -> ApiResult<SignInTurn<'a>> {
    refuse_if_throttled(state, username, source).await?;
    let slot = state.sign_in_queue.try_acquire().map_err(|_| {
        ApiError::too_many(
            "sign_in_busy",
            "too many sign-ins at once; try again shortly",
            SignedDuration::from_secs(1),
        )
    })?;
    let gate = state.sign_in_gate.lock().await;
    // The pre-check is stale after the wait: failures recorded meanwhile count.
    refuse_if_throttled(state, username, source).await?;
    Ok(SignInTurn {
        _slot: slot,
        _gate: gate,
    })
}

async fn refuse_if_throttled(state: &AppState, username: &str, source: &str) -> ApiResult<()> {
    match throttle::check(&state.pool, username, source, state.clock.now()).await? {
        Verdict::Allowed => Ok(()),
        Verdict::AccountLocked { retry_after } => Err(ApiError::too_many(
            "account_locked",
            "too many failed sign-ins; this account is locked for a while",
            retry_after,
        )),
        Verdict::SourceThrottled { retry_after } => Err(ApiError::too_many(
            "too_many_attempts",
            "too many failed sign-ins from this device; try again later",
            retry_after,
        )),
    }
}

/// Checks a signed-in user's current password before a sensitive change. A wrong
/// password counts as a failed sign-in; a right one clears earlier failures.
pub async fn confirm_password(
    state: &AppState,
    source: &str,
    user: &User,
    password: String,
) -> ApiResult<()> {
    let _turn = sign_in_turn(state, &user.username, source).await?;
    check_password(state, source, user, password).await?;
    throttle::record_success(&state.pool, &user.username).await?;
    Ok(())
}

/// Like [`confirm_password`], and also checks a TOTP code when the user has TOTP
/// on. Failures are cleared only once both match, so a known password cannot
/// guess codes past the lockout.
pub async fn confirm_password_and_code(
    state: &AppState,
    source: &str,
    user: &User,
    password: String,
    code: Option<&str>,
) -> ApiResult<()> {
    let _turn = sign_in_turn(state, &user.username, source).await?;
    check_password(state, source, user, password).await?;
    if let Some(sealed) = user.totp_secret.as_deref() {
        totp::check_second_factor(state, user, sealed, code, source).await?;
    }
    throttle::record_success(&state.pool, &user.username).await?;
    Ok(())
}

async fn check_password(
    state: &AppState,
    source: &str,
    user: &User,
    password: String,
) -> ApiResult<()> {
    if password::verify(password, user.password_hash.clone()).await {
        return Ok(());
    }
    throttle::record_failure(&state.pool, &user.username, source, state.clock.now()).await?;
    Err(ApiError::forbidden(
        "wrong_password",
        "the current password is incorrect",
    ))
}
