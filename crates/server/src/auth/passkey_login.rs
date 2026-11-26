//! Discoverable sign-in with a passkey.
//!
//! The browser offers the passkeys it holds for this site, and the chosen one
//! names its user through the `WebAuthn` user handle. A passkey verifies the user on
//! the device, so it skips the TOTP prompt.
//!
//! Both calls are anonymous. A failed sign-in counts toward the per-client brake
//! that password sign-in uses, and a client over that brake is refused. An account
//! lockout does not stop a passkey: the lockout stops password guessing, a passkey
//! cannot be guessed, and so the owner still gets in while someone guesses their
//! password.

use axum::{Json, extract::State, http::StatusCode, response::Response};
use saffron_db::{
    passkeys,
    users::{self, User},
};
use serde::Deserialize;
use uuid::Uuid;
use webauthn_rs::prelude::{Passkey, PublicKeyCredential, RequestChallengeResponse};

use super::{
    extract::ClientSource,
    passkeys::{Challenge, ceremony_expired, entry, relying_party},
    routes::signed_in,
    throttle,
};
use crate::{
    error::{ApiError, ApiResult},
    request::JsonBody,
    state::AppState,
    users::record_audit,
};

/// Body of `POST /api/auth/passkey/login/finish`.
#[derive(Debug, Deserialize)]
pub struct LoginFinishRequest {
    ceremony_id: Uuid,
    credential: PublicKeyCredential,
}

/// `POST /api/auth/passkey/login/start`: a challenge any of this site's passkeys
/// can answer.
pub async fn login_start(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
) -> ApiResult<Json<Challenge<RequestChallengeResponse>>> {
    let webauthn = relying_party(&state)?;
    refuse_if_braked(&state, &source).await?;
    let (options, authentication) = webauthn
        .start_discoverable_authentication()
        .map_err(ApiError::internal)?;
    let ceremony_id =
        state
            .ceremonies
            .insert_authentication(authentication, &source, state.clock.now());
    Ok(Json(Challenge {
        ceremony_id,
        options,
    }))
}

/// `POST /api/auth/passkey/login/finish`: signs in with the passkey's answer.
pub async fn login_finish(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
    JsonBody(request): JsonBody<LoginFinishRequest>,
) -> ApiResult<Response> {
    let webauthn = relying_party(&state)?;
    refuse_if_braked(&state, &source).await?;
    let now = state.clock.now();
    let authentication = state
        .ceremonies
        .take_authentication(request.ceremony_id, now)
        .ok_or_else(ceremony_expired)?;
    let Ok((handle, credential_id)) =
        webauthn.identify_discoverable_authentication(&request.credential)
    else {
        return refuse(&state, &source, None).await;
    };
    let user = users::by_webauthn_id(&state.pool, handle.as_bytes())
        .await?
        .filter(User::is_active);
    let stored = match &user {
        Some(user) => passkeys::by_credential_id(&state.pool, credential_id)
            .await?
            .filter(|passkey| passkey.user_id == user.id),
        None => None,
    };
    let (Some(user), Some(stored)) = (user, stored) else {
        return refuse(&state, &source, None).await;
    };
    let mut passkey: Passkey =
        serde_json::from_str(&stored.credential).map_err(ApiError::internal)?;
    let result = match webauthn.finish_discoverable_authentication_v2(
        &request.credential,
        authentication,
        &passkey,
    ) {
        Ok(result) => result,
        Err(error) => {
            tracing::info!(%error, user = %user.username, "passkey sign-in refused");
            return refuse(&state, &source, Some(&user)).await;
        }
    };
    passkey.update_credential(&result);
    let credential = serde_json::to_string(&passkey).map_err(ApiError::internal)?;
    passkeys::record_use(&state.pool, stored.id, &credential, now).await?;
    record_audit(&state, entry(Some(&user), "auth.passkey_login", None)).await;
    signed_in(&state, &user, StatusCode::OK).await
}

async fn refuse_if_braked(state: &AppState, source: &str) -> ApiResult<()> {
    match throttle::source_brake(&state.pool, source, state.clock.now()).await? {
        Some(retry_after) => Err(ApiError::too_many(
            "too_many_attempts",
            "too many failed sign-ins from this device; try again later",
            retry_after,
        )),
        None => Ok(()),
    }
}

/// Counts a refused sign-in toward the per-client brake. Only a refusal for a known
/// user is audited: anonymous garbage, which the brake bounds anyway, would
/// otherwise fill the audit log.
async fn refuse(state: &AppState, source: &str, user: Option<&User>) -> ApiResult<Response> {
    throttle::record_source_failure(&state.pool, source, state.clock.now()).await?;
    if let Some(user) = user {
        record_audit(state, entry(Some(user), "auth.passkey_failed", None)).await;
    } else {
        tracing::info!(%source, "passkey sign-in refused for an unknown passkey");
    }
    Err(ApiError::new(
        StatusCode::UNAUTHORIZED,
        "passkey_failed",
        "this passkey could not sign you in",
    ))
}
