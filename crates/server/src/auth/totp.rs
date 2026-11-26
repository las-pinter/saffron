//! Time-based one-time codes (RFC 6238) as an optional second factor for password
//! sign-in.
//!
//! Enrolment is two steps: `setup` offers a new secret, and `confirm` turns TOTP on
//! once a code from the authenticator app proves the secret was saved. Secrets are
//! sealed with the server key, and each 30-second step is accepted only once.

use axum::{Json, extract::State, http::StatusCode};
use jiff::Timestamp;
use saffron_db::{
    audit, sessions,
    users::{self, User},
};
use serde::{Deserialize, Serialize};
use totp_rs::{Algorithm, Builder, Secret, Totp};

use super::{
    extract::{AdminUser, ClientSource, CurrentUser},
    gate::confirm_password,
    throttle,
};
use crate::{
    error::{ApiError, ApiResult},
    request::{JsonBody, PathParam},
    secrets::totp_purpose,
    state::AppState,
    users::record_audit,
};

/// Issuer shown in authenticator apps.
pub const ISSUER: &str = "Saffron";
/// Secret length in bytes (160 bits, as RFC 4226 recommends).
const SEED_BYTES: usize = 20;

/// Body of `POST /api/auth/totp/setup` and `POST /api/auth/totp/disable`.
#[derive(Debug, Deserialize)]
pub struct PasswordRequest {
    password: String,
}

/// Body of `POST /api/auth/totp/confirm`.
#[derive(Debug, Deserialize)]
pub struct CodeRequest {
    code: String,
}

/// The secret offered during enrolment.
#[derive(Debug, Serialize, Deserialize)]
pub struct TotpOffer {
    /// The secret in base32, for typing into an authenticator app.
    pub secret: String,
    /// An `otpauth://` URL, for a QR code.
    pub otpauth_url: String,
}

fn generator(seed: Vec<u8>, account: &str) -> Result<Totp, ApiError> {
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(Secret::from(seed))
        .with_issuer(Some(ISSUER))
        .with_account_name(account)
        .build()
        .map_err(ApiError::internal)
}

/// The step `code` belongs to at `now`, allowing one step of clock drift either way.
fn matching_step(totp: &Totp, code: &str, now: Timestamp) -> Option<i64> {
    let seconds = u64::try_from(now.as_second()).ok()?;
    totp.check(code.trim(), seconds)
        .and_then(|step| i64::try_from(step).ok())
}

fn open_seed(state: &AppState, user_id: i64, sealed: &str) -> Result<Vec<u8>, ApiError> {
    state
        .secrets
        .open(sealed, &totp_purpose(user_id))
        .map_err(ApiError::internal)
}

/// `POST /api/auth/totp/setup`: offers a new secret. Needs the current password, so
/// a stolen session cannot lock the owner out with a second factor of its own.
pub async fn setup(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<PasswordRequest>,
) -> ApiResult<Json<TotpOffer>> {
    let user = &session.user;
    if user.totp_secret.is_some() {
        return Err(ApiError::conflict(
            "totp_already_enabled",
            "two-factor sign-in is already on; turn it off first",
        ));
    }
    confirm_password(&state, &source, user, request.password).await?;
    let mut seed = vec![0_u8; SEED_BYTES];
    getrandom::fill(&mut seed).map_err(ApiError::internal)?;
    let sealed = state
        .secrets
        .seal(&seed, &totp_purpose(user.id))
        .map_err(ApiError::internal)?;
    users::set_totp_pending(&state.pool, user.id, Some(&sealed)).await?;
    let totp = generator(seed, &user.username)?;
    Ok(Json(TotpOffer {
        secret: totp.secret().to_base32(),
        otpauth_url: totp.to_url().map_err(ApiError::internal)?,
    }))
}

/// `POST /api/auth/totp/confirm`: turns TOTP on when `code` matches the offered
/// secret.
pub async fn confirm(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<CodeRequest>,
) -> ApiResult<StatusCode> {
    let user = &session.user;
    let Some(sealed) = user.totp_pending.as_deref() else {
        return Err(ApiError::bad_request(
            "no_totp_pending",
            "start two-factor setup first",
        ));
    };
    let totp = generator(open_seed(&state, user.id, sealed)?, &user.username)?;
    let Some(step) = matching_step(&totp, &request.code, state.clock.now()) else {
        return Err(ApiError::bad_request(
            "invalid_code",
            "the code does not match; check the app's clock and try the next code",
        ));
    };
    if !users::enable_totp(&state.pool, user.id, sealed, step).await? {
        return Err(ApiError::conflict(
            "totp_setup_changed",
            "two-factor setup changed meanwhile; start it again",
        ));
    }
    record_audit(&state, entry(user, user, "auth.totp_enable")).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/auth/totp/disable`: turns TOTP off. Needs the current password.
pub async fn disable(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<PasswordRequest>,
) -> ApiResult<StatusCode> {
    let user = &session.user;
    confirm_password(&state, &source, user, request.password).await?;
    users::disable_totp(&state.pool, user.id).await?;
    record_audit(&state, entry(user, user, "auth.totp_disable")).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/users/{id}/totp/reset`: an administrator turns a user's TOTP off, for
/// a lost authenticator. Ends the user's sessions, since one of them may be the
/// thief who enrolled. An administrator's own TOTP is refused here: turning it off
/// needs the password, which a stolen session lacks.
pub async fn reset(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    PathParam(id): PathParam<i64>,
) -> ApiResult<StatusCode> {
    if id == admin.user.id {
        return Err(ApiError::bad_request(
            "own_totp",
            "turn off your own two-factor sign-in with your password instead",
        ));
    }
    let target = users::by_id(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    users::disable_totp(&state.pool, id).await?;
    sessions::delete_for_user(&state.pool, id, None).await?;
    record_audit(&state, entry(&admin.user, &target, "user.totp_reset")).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Checks the second factor of a password sign-in whose password already matched.
///
/// A missing code answers `totp_required` and counts as nothing; a wrong or reused
/// code counts as a failed sign-in.
pub async fn check_second_factor(
    state: &AppState,
    user: &User,
    sealed: &str,
    code: Option<&str>,
    source: &str,
) -> ApiResult<()> {
    let Some(code) = code.filter(|code| !code.trim().is_empty()) else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "totp_required",
            "enter the code from your authenticator app",
        ));
    };
    let now = state.clock.now();
    let totp = generator(open_seed(state, user.id, sealed)?, &user.username)?;
    let accepted = match matching_step(&totp, code, now) {
        Some(step) => users::claim_totp_step(&state.pool, user.id, step).await?,
        None => false,
    };
    if !accepted {
        throttle::record_failure(&state.pool, &user.username, source, now).await?;
        record_audit(state, entry(user, user, "auth.totp_failed")).await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_code",
            "the code is wrong or was already used",
        ));
    }
    Ok(())
}

fn entry<'a>(actor: &'a User, target: &'a User, action: &'a str) -> audit::Entry<'a> {
    audit::Entry {
        user_id: Some(actor.id),
        action,
        target: Some(&target.username),
        detail: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totp() -> Totp {
        generator(b"12345678901234567890".to_vec(), "owner").expect("generator")
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(seconds).expect("timestamp")
    }

    #[test]
    fn the_rfc_6238_test_vector_matches() {
        // RFC 6238 appendix B: SHA-1 at 59 s gives 94287082; six digits keep 287082.
        assert_eq!(matching_step(&totp(), "287082", at(59)), Some(1));
    }

    #[test]
    fn a_code_from_the_previous_step_is_accepted() {
        assert_eq!(matching_step(&totp(), "287082", at(89)), Some(1));
    }

    #[test]
    fn a_code_two_steps_old_is_refused() {
        assert_eq!(matching_step(&totp(), "287082", at(119)), None);
    }

    #[test]
    fn surrounding_spaces_are_ignored() {
        assert_eq!(matching_step(&totp(), " 287082 ", at(59)), Some(1));
    }

    #[test]
    fn a_wrong_code_is_refused() {
        assert_eq!(matching_step(&totp(), "000000", at(59)), None);
    }

    #[test]
    fn the_otpauth_url_names_issuer_and_account() {
        let url = totp().to_url().expect("url");

        assert!(url.starts_with("otpauth://totp/Saffron:owner?"), "{url}");
        assert!(url.contains("issuer=Saffron"), "{url}");
    }
}
