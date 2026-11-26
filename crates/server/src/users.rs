//! User administration: administrators list, create, disable, and enable users and
//! reset their passwords. Users are disabled, never deleted, because deleting one
//! would delete their portfolios.

use axum::{Json, extract::State, http::StatusCode};
use saffron_db::{
    audit, is_unique_violation, sessions,
    users::{self, NewUser, User},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{extract::AdminUser, password, throttle},
    error::{ApiError, ApiResult},
    request::{JsonBody, PathParam},
    state::AppState,
};

/// Longest username in characters.
pub const MAX_USERNAME_CHARS: usize = 64;
/// Longest display name in characters.
pub const MAX_DISPLAY_NAME_CHARS: usize = 100;

/// A user as the API shows it: never the password hash or the TOTP secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserView {
    /// Row id.
    pub id: i64,
    /// Sign-in name.
    pub username: String,
    /// Name shown in the interface.
    pub display_name: String,
    /// Whether the user manages other users.
    pub is_admin: bool,
    /// Whether the account is disabled.
    pub disabled: bool,
    /// Whether password sign-in also needs a TOTP code.
    pub totp_enabled: bool,
}

impl From<&User> for UserView {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            is_admin: user.is_admin,
            disabled: !user.is_active(),
            totp_enabled: user.totp_secret.is_some(),
        }
    }
}

/// Body of `POST /api/users`.
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    username: String,
    display_name: String,
    password: String,
    #[serde(default)]
    is_admin: bool,
}

/// Body of `POST /api/users/{id}/password`.
#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    password: String,
}

/// A trimmed username of 1 to 64 ASCII letters, digits, `.`, `_`, or `-`.
pub fn validate_username(raw: &str) -> Result<String, ApiError> {
    let username = raw.trim();
    let valid = !username.is_empty()
        && username.chars().count() <= MAX_USERNAME_CHARS
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(ApiError::bad_request(
            "invalid_username",
            "a username has 1 to 64 letters, digits, dots, underscores, or hyphens",
        ));
    }
    Ok(username.to_owned())
}

/// A trimmed, non-empty display name of at most 100 characters.
pub fn validate_display_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(ApiError::bad_request(
            "invalid_display_name",
            "a display name has 1 to 100 characters",
        ));
    }
    Ok(name.to_owned())
}

/// Hashes a new password, turning rule violations into 400 responses.
pub async fn hash_new_password(password: String) -> Result<String, ApiError> {
    password::hash(password).await.map_err(|error| match error {
        password::PasswordError::Hash(_) => ApiError::internal(error),
        _ => ApiError::bad_request("invalid_password", error.to_string()),
    })
}

/// Appends an audit entry; a failure is logged, never shown to the client.
pub async fn record_audit(state: &AppState, entry: audit::Entry<'_>) {
    if let Err(error) = audit::record(&state.pool, state.clock.now(), entry).await {
        tracing::error!(%error, action = entry.action, "cannot write the audit log");
    }
}

/// `GET /api/users`
pub async fn list(State(state): State<AppState>, _: AdminUser) -> ApiResult<Json<Vec<UserView>>> {
    let users = users::list(&state.pool).await?;
    Ok(Json(users.iter().map(UserView::from).collect()))
}

/// `POST /api/users`
pub async fn create(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    JsonBody(request): JsonBody<CreateUserRequest>,
) -> ApiResult<(StatusCode, Json<UserView>)> {
    let username = validate_username(&request.username)?;
    let display_name = validate_display_name(&request.display_name)?;
    let password_hash = hash_new_password(request.password).await?;
    let new_user = NewUser {
        username: &username,
        display_name: &display_name,
        is_admin: request.is_admin,
        password_hash: Some(&password_hash),
    };
    let id = match users::create(&state.pool, new_user, state.clock.now()).await {
        Ok(id) => id,
        Err(error) if is_unique_violation(&error) => {
            return Err(ApiError::conflict(
                "username_taken",
                "another user has this username",
            ));
        }
        Err(error) => return Err(error.into()),
    };
    record_audit(
        &state,
        audit::Entry {
            user_id: Some(admin.user.id),
            action: "user.create",
            target: Some(&username),
            detail: None,
        },
    )
    .await;
    let user = users::by_id(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(UserView::from(&user))))
}

/// `POST /api/users/{id}/disable`: also ends the user's sessions.
pub async fn disable(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    PathParam(id): PathParam<i64>,
) -> ApiResult<StatusCode> {
    if id == admin.user.id {
        return Err(ApiError::bad_request(
            "cannot_disable_self",
            "an administrator cannot disable their own account",
        ));
    }
    let user = existing_user(&state, id).await?;
    users::set_disabled(&state.pool, id, Some(state.clock.now())).await?;
    sessions::delete_for_user(&state.pool, id, None).await?;
    audit_admin_action(&state, &admin.user, "user.disable", &user).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/users/{id}/enable`: also lifts a sign-in lock.
pub async fn enable(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    PathParam(id): PathParam<i64>,
) -> ApiResult<StatusCode> {
    let user = existing_user(&state, id).await?;
    users::set_disabled(&state.pool, id, None).await?;
    throttle::record_success(&state.pool, &user.username).await?;
    audit_admin_action(&state, &admin.user, "user.enable", &user).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/users/{id}/password`: sets a new password and ends the user's
/// sessions.
pub async fn reset_password(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    PathParam(id): PathParam<i64>,
    JsonBody(request): JsonBody<ResetPasswordRequest>,
) -> ApiResult<StatusCode> {
    let user = existing_user(&state, id).await?;
    let password_hash = hash_new_password(request.password).await?;
    users::set_password_hash(&state.pool, id, &password_hash).await?;
    sessions::delete_for_user(&state.pool, id, None).await?;
    audit_admin_action(&state, &admin.user, "user.reset_password", &user).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn existing_user(state: &AppState, id: i64) -> Result<User, ApiError> {
    users::by_id(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)
}

async fn audit_admin_action(state: &AppState, admin: &User, action: &str, target: &User) {
    record_audit(
        state,
        audit::Entry {
            user_id: Some(admin.id),
            action,
            target: Some(&target.username),
            detail: None,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_username_is_trimmed() {
        assert_eq!(validate_username("  las.p-1_ "), Ok("las.p-1_".to_owned()));
    }

    #[test]
    fn a_username_with_a_space_is_rejected() {
        assert!(validate_username("las p").is_err());
    }

    #[test]
    fn an_empty_username_is_rejected() {
        assert!(validate_username("   ").is_err());
    }

    #[test]
    fn a_username_of_65_characters_is_rejected() {
        assert!(validate_username(&"a".repeat(MAX_USERNAME_CHARS + 1)).is_err());
    }

    #[test]
    fn a_username_of_64_characters_is_accepted() {
        assert!(validate_username(&"a".repeat(MAX_USERNAME_CHARS)).is_ok());
    }

    #[test]
    fn a_display_name_may_hold_any_characters() {
        assert_eq!(
            validate_display_name(" Lász Pintér "),
            Ok("Lász Pintér".to_owned())
        );
    }

    #[test]
    fn a_blank_display_name_is_rejected() {
        assert!(validate_display_name(" ").is_err());
    }

    #[test]
    fn a_display_name_of_101_characters_is_rejected() {
        assert!(validate_display_name(&"a".repeat(MAX_DISPLAY_NAME_CHARS + 1)).is_err());
    }
}
