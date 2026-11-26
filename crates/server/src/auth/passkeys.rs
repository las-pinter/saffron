//! Passkeys: registration, listing, and removal. Sign-in with a passkey lives in
//! [`super::passkey_login`].

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use saffron_db::{
    audit, is_unique_violation,
    passkeys::{self, NewPasskey},
    users::{self, User},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, RegisterPublicKeyCredential, Webauthn,
};

use webauthn_rs_proto::ResidentKeyRequirement;

use super::{
    ceremony::Registration,
    extract::{ClientSource, CurrentUser},
    gate::confirm_password_and_code,
};
use crate::{
    error::{ApiError, ApiResult},
    request::{JsonBody, PathParam},
    state::AppState,
    users::record_audit,
};

/// Longest passkey name, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// Body of `POST /api/auth/passkeys/register/start`.
#[derive(Debug, Deserialize)]
pub struct RegisterStartRequest {
    password: String,
    /// A current code; needed while TOTP is on.
    #[serde(default)]
    code: Option<String>,
}

/// A started ceremony: its id and the options for the browser.
#[derive(Debug, Serialize, Deserialize)]
pub struct Challenge<T> {
    /// Sent back with the finish call.
    pub ceremony_id: Uuid,
    /// Passed to `navigator.credentials`.
    pub options: T,
}

/// Body of `POST /api/auth/passkeys/register/finish`.
#[derive(Debug, Deserialize)]
pub struct RegisterFinishRequest {
    ceremony_id: Uuid,
    name: String,
    credential: RegisterPublicKeyCredential,
}

/// A passkey as the API shows it; the credential itself stays on the server.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PasskeyView {
    /// Row id, for removal.
    pub id: i64,
    /// Name the user gave it.
    pub name: String,
    /// When it was registered.
    pub created_at: String,
    /// When it last signed in.
    pub last_used_at: Option<String>,
}

impl From<passkeys::Passkey> for PasskeyView {
    fn from(passkey: passkeys::Passkey) -> Self {
        Self {
            id: passkey.id,
            name: passkey.name,
            created_at: passkey.created_at,
            last_used_at: passkey.last_used_at,
        }
    }
}

pub(super) fn relying_party(state: &AppState) -> ApiResult<&Webauthn> {
    state.webauthn.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "passkeys_unavailable",
            "passkeys need SAFFRON_PUBLIC_URL to name a host, not an IP address",
        )
    })
}

/// `POST /api/auth/passkeys/register/start`: offers a new passkey to the browser.
/// Needs the password and, while TOTP is on, a code, so a stolen session cannot add
/// a lasting way in.
pub async fn register_start(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<RegisterStartRequest>,
) -> ApiResult<Json<Challenge<CreationChallengeResponse>>> {
    let webauthn = relying_party(&state)?;
    let user = &session.user;
    confirm_password_and_code(
        &state,
        &source,
        user,
        request.password,
        request.code.as_deref(),
    )
    .await?;
    let handle = users::webauthn_id_or_set(&state.pool, user.id, Uuid::new_v4().as_bytes())
        .await?
        .ok_or_else(ApiError::not_found)?;
    let handle = Uuid::from_slice(&handle).map_err(ApiError::internal)?;
    let registered = passkeys::for_user(&state.pool, user.id)
        .await?
        .into_iter()
        .map(|passkey| CredentialID::from(passkey.credential_id))
        .collect();
    let (mut options, registration) = webauthn
        .start_passkey_registration(handle, &user.username, &user.display_name, Some(registered))
        .map_err(ApiError::internal)?;
    require_discoverable(&mut options);
    let ceremony_id = state.ceremonies.insert_registration(
        Registration {
            user_id: user.id,
            token_hash: session.token_hash,
            state: registration,
        },
        state.clock.now(),
    );
    Ok(Json(Challenge {
        ceremony_id,
        options,
    }))
}

/// Asks the authenticator for a discoverable credential, which sign-in needs.
/// `webauthn-rs` discourages one for passkeys and does not check the flag when the
/// registration finishes, so asking here is safe.
fn require_discoverable(options: &mut CreationChallengeResponse) {
    if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }
}

/// `POST /api/auth/passkeys/register/finish`: stores the passkey the browser made.
pub async fn register_finish(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<RegisterFinishRequest>,
) -> ApiResult<Response> {
    let webauthn = relying_party(&state)?;
    let user = &session.user;
    let name = validate_name(&request.name)?;
    let now = state.clock.now();
    let registration = state
        .ceremonies
        .take_registration(request.ceremony_id, now)
        .filter(|registration| {
            registration.user_id == user.id && registration.token_hash == session.token_hash
        })
        .ok_or_else(ceremony_expired)?;
    // The library refuses a credential on the exclude list with a misleading error;
    // answer it plainly first.
    if passkeys::by_credential_id(&state.pool, &request.credential.raw_id)
        .await?
        .is_some()
    {
        return Err(passkey_exists());
    }
    let passkey = webauthn
        .finish_passkey_registration(&request.credential, &registration.state)
        .map_err(|error| {
            tracing::info!(%error, user = %user.username, "passkey registration refused");
            ApiError::bad_request("passkey_rejected", "the passkey could not be verified")
        })?;
    let credential = serde_json::to_string(&passkey).map_err(ApiError::internal)?;
    let new = NewPasskey {
        user_id: user.id,
        credential_id: passkey.cred_id().as_ref(),
        credential: &credential,
        name,
    };
    let id = passkeys::insert(&state.pool, new, now)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                passkey_exists()
            } else {
                error.into()
            }
        })?;
    let detail = serde_json::json!({ "name": name }).to_string();
    record_audit(&state, entry(Some(user), "auth.passkey_add", Some(&detail))).await;
    let view = PasskeyView {
        id,
        name: name.to_owned(),
        created_at: saffron_db::time::to_text(now),
        last_used_at: None,
    };
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

fn passkey_exists() -> ApiError {
    ApiError::conflict("passkey_exists", "this passkey is already registered")
}

fn validate_name(raw: &str) -> ApiResult<&str> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request(
            "invalid_name",
            format!("a passkey name has 1 to {MAX_NAME_CHARS} characters"),
        ));
    }
    Ok(name)
}

pub(super) fn ceremony_expired() -> ApiError {
    ApiError::bad_request(
        "ceremony_expired",
        "the passkey request expired or was already used; start again",
    )
}

/// `GET /api/auth/passkeys`: the signed-in user's passkeys.
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
) -> ApiResult<Json<Vec<PasskeyView>>> {
    let listed = passkeys::for_user(&state.pool, session.user.id).await?;
    Ok(Json(listed.into_iter().map(PasskeyView::from).collect()))
}

/// `DELETE /api/auth/passkeys/{id}`: removes one of the signed-in user's passkeys.
///
/// Unlike adding one, this needs no password: a stolen session gains no way in by
/// removing a passkey, and the password still signs the owner in.
pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    PathParam(id): PathParam<i64>,
) -> ApiResult<StatusCode> {
    let user = &session.user;
    if !passkeys::delete(&state.pool, user.id, id).await? {
        return Err(ApiError::not_found());
    }
    record_audit(&state, entry(Some(user), "auth.passkey_remove", None)).await;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn entry<'a>(
    user: Option<&'a User>,
    action: &'a str,
    detail: Option<&'a str>,
) -> audit::Entry<'a> {
    audit::Entry {
        user_id: user.map(|user| user.id),
        action,
        target: user.map(|user| user.username.as_str()),
        detail,
    }
}
