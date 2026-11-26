//! Sign-in endpoints: first-run setup, password login, logout, and password change.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header::SET_COOKIE},
    response::{AppendHeaders, IntoResponse, Response},
};
use jiff::SignedDuration;
use saffron_db::{
    audit, sessions,
    users::{self, NewUser, User},
};
use serde::{Deserialize, Serialize};
use tokio::sync::{MutexGuard, SemaphorePermit};

use super::{
    cookie,
    extract::{ClientSource, CurrentUser},
    password, session,
    throttle::{self, Verdict},
};
use crate::{
    error::{ApiError, ApiResult},
    request::JsonBody,
    state::AppState,
    users::{
        MAX_USERNAME_CHARS, UserView, hash_new_password, record_audit, validate_display_name,
        validate_username,
    },
};

/// Body of `GET /api/auth/state`.
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthState {
    /// No user exists yet; the setup link from the log creates the first one.
    pub setup_required: bool,
    /// The signed-in user, if any.
    pub user: Option<UserView>,
}

/// Body of `POST /api/auth/setup`.
#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    token: String,
    username: String,
    display_name: String,
    password: String,
}

/// Body of `POST /api/auth/login`.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

/// Body of `POST /api/auth/password`.
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Body of a successful setup or login.
#[derive(Debug, Serialize, Deserialize)]
pub struct SignedIn {
    /// The signed-in user.
    pub user: UserView,
}

/// `GET /api/auth/state`
pub async fn state(
    State(state): State<AppState>,
    current: Option<CurrentUser>,
) -> ApiResult<Json<AuthState>> {
    Ok(Json(AuthState {
        setup_required: users::count(&state.pool).await? == 0,
        user: current.map(|CurrentUser(session)| UserView::from(&session.user)),
    }))
}

/// `POST /api/auth/setup`: creates the first administrator and signs them in.
pub async fn setup(
    State(state): State<AppState>,
    JsonBody(request): JsonBody<SetupRequest>,
) -> ApiResult<Response> {
    if !state.setup.matches(&request.token) {
        return Err(ApiError::forbidden(
            "invalid_setup_token",
            "the setup link is invalid; use the newest link from the server log",
        ));
    }
    let username = validate_username(&request.username)?;
    let display_name = validate_display_name(&request.display_name)?;
    let password_hash = hash_new_password(request.password).await?;
    let new_user = NewUser {
        username: &username,
        display_name: &display_name,
        is_admin: true,
        password_hash: Some(&password_hash),
    };
    let now = state.clock.now();
    let Some(id) = users::create_first(&state.pool, new_user, now).await? else {
        return Err(ApiError::conflict(
            "already_set_up",
            "Saffron is already set up",
        ));
    };
    state.setup.clear();
    let user = users::by_id(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    record_audit(&state, user_entry(&user, "auth.setup")).await;
    signed_in(&state, &user, StatusCode::CREATED).await
}

/// `POST /api/auth/login`: signs in with username and password.
pub async fn login(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
    JsonBody(request): JsonBody<LoginRequest>,
) -> ApiResult<Response> {
    let username = throttle_key(&request.username);
    let username = username.as_str();
    let _turn = sign_in_turn(&state, username, &source).await?;
    let now = state.clock.now();

    let user = users::by_username(&state.pool, username)
        .await?
        .filter(User::is_active);
    let stored = user.as_ref().and_then(|user| user.password_hash.clone());
    let matched = password::verify(request.password, stored).await;
    let Some(user) = user.filter(|_| matched) else {
        let locked = throttle::record_failure(&state.pool, username, &source, now).await?;
        let detail = locked.then_some(r#"{"locked":true}"#);
        record_audit(
            &state,
            audit::Entry {
                user_id: None,
                action: "auth.login_failed",
                target: Some(username),
                detail,
            },
        )
        .await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "the username or password is incorrect",
        ));
    };

    throttle::record_success(&state.pool, &user.username).await?;
    record_audit(&state, user_entry(&user, "auth.login")).await;
    signed_in(&state, &user, StatusCode::OK).await
}

/// `POST /api/auth/logout`
pub async fn logout(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
) -> ApiResult<impl IntoResponse> {
    sessions::delete(&state.pool, &session.token_hash).await?;
    record_audit(&state, user_entry(&session.user, "auth.logout")).await;
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(SET_COOKIE, cookie::clear(&state.origin))]),
    ))
}

/// `POST /api/auth/password`: changes the caller's password and ends their other
/// sessions. A wrong current password counts as a failed sign-in.
pub async fn change_password(
    State(state): State<AppState>,
    ClientSource(source): ClientSource,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<ChangePasswordRequest>,
) -> ApiResult<StatusCode> {
    let user = &session.user;
    let turn = sign_in_turn(&state, &user.username, &source).await?;
    if !password::verify(request.current_password, user.password_hash.clone()).await {
        throttle::record_failure(&state.pool, &user.username, &source, state.clock.now()).await?;
        return Err(ApiError::forbidden(
            "wrong_password",
            "the current password is incorrect",
        ));
    }
    throttle::record_success(&state.pool, &user.username).await?;
    drop(turn);
    let password_hash = hash_new_password(request.new_password).await?;
    users::set_password_hash(&state.pool, user.id, &password_hash).await?;
    sessions::delete_for_user(&state.pool, user.id, Some(&session.token_hash)).await?;
    record_audit(&state, user_entry(user, "auth.change_password")).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The submitted username as the throttle records it: trimmed, and cut to one
/// character more than any valid username, so a huge value cannot bloat the table.
fn throttle_key(raw: &str) -> String {
    raw.trim().chars().take(MAX_USERNAME_CHARS + 1).collect()
}

/// Held while one password check and its outcome are recorded.
struct SignInTurn<'a> {
    _slot: SemaphorePermit<'a>,
    _gate: MutexGuard<'a, ()>,
}

/// Waits for this request's turn to check a password, so parallel guesses cannot
/// overshoot the lockout and only one Argon2 verification runs at a time.
///
/// A throttled client is refused before it joins the queue, and the queue is
/// bounded, so a flood cannot hold up everyone else's sign-in for long.
async fn sign_in_turn<'a>(
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

async fn signed_in(state: &AppState, user: &User, status: StatusCode) -> ApiResult<Response> {
    let token = session::start(&state.pool, user.id, state.clock.now()).await?;
    let set_cookie = cookie::set(&state.origin, &token, session::cookie_max_age_secs());
    Ok((
        status,
        AppendHeaders([(SET_COOKIE, set_cookie)]),
        Json(SignedIn {
            user: UserView::from(user),
        }),
    )
        .into_response())
}

fn user_entry<'a>(user: &'a User, action: &'a str) -> audit::Entry<'a> {
    audit::Entry {
        user_id: Some(user.id),
        action,
        target: Some(&user.username),
        detail: None,
    }
}
