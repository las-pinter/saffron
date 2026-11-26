//! Sharing a portfolio. The owner shares it with other users as a viewer or an
//! editor; a user it is shared with may leave it.

use axum::{Json, extract::State, http::StatusCode};
use saffron_db::{
    portfolios::{self, Role, ShareRole},
    users::{self, User},
};
use serde::{Deserialize, Serialize};

use super::{entry, guard::PortfolioAccess};
use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    request::{JsonBody, PathParam},
    state::AppState,
    users::record_audit,
};

/// Body of `POST /api/portfolios/{portfolio_id}/shares`.
#[derive(Debug, Deserialize)]
pub struct ShareRequest {
    username: String,
    role: ShareRoleName,
}

/// The role named in a share request.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareRoleName {
    /// Reads the portfolio.
    Viewer,
    /// Reads and changes its contents.
    Editor,
}

impl From<ShareRoleName> for ShareRole {
    fn from(role: ShareRoleName) -> Self {
        match role {
            ShareRoleName::Viewer => Self::Viewer,
            ShareRoleName::Editor => Self::Editor,
        }
    }
}

/// A share as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareView {
    /// The user's id.
    pub user_id: i64,
    /// The user's sign-in name.
    pub username: String,
    /// The user's display name.
    pub display_name: String,
    /// `viewer` or `editor`.
    pub role: String,
    /// When it was shared.
    pub created_at: String,
}

/// `GET /api/portfolios/{portfolio_id}/shares`: who it is shared with. Owner only.
pub async fn list(
    State(state): State<AppState>,
    access: PortfolioAccess,
) -> ApiResult<Json<Vec<ShareView>>> {
    access.require(Role::Owner)?;
    let shares = portfolios::shares(&state.pool, access.0.portfolio.id).await?;
    Ok(Json(
        shares
            .into_iter()
            .map(|share| ShareView {
                user_id: share.user_id,
                username: share.username,
                display_name: share.display_name,
                role: share.role.as_str().to_owned(),
                created_at: share.created_at,
            })
            .collect(),
    ))
}

/// `POST /api/portfolios/{portfolio_id}/shares`: shares it with a user, or changes
/// their role. Owner only.
pub async fn put(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<ShareRequest>,
) -> ApiResult<StatusCode> {
    access.require(Role::Owner)?;
    let target = users::by_username(&state.pool, request.username.trim())
        .await?
        .filter(User::is_active)
        .ok_or_else(|| ApiError::bad_request("unknown_user", "no active user has that name"))?;
    if target.id == access.0.portfolio.owner_id {
        return Err(ApiError::bad_request(
            "owner_share",
            "the owner already has full access",
        ));
    }
    let role = ShareRole::from(request.role);
    portfolios::share(
        &state.pool,
        access.0.portfolio.id,
        target.id,
        role,
        state.clock.now(),
    )
    .await?;
    let detail = serde_json::json!({
        "user": target.username,
        "role": Role::from(role).as_str(),
    })
    .to_string();
    record_audit(
        &state,
        entry(&session.user, &access.0, "portfolio.share", Some(&detail)),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/portfolios/{portfolio_id}/shares/{user_id}`: stops sharing it with a
/// user. Owner only; see [`leave`] for a user leaving on their own.
pub async fn remove(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    PathParam((_, user_id)): PathParam<(i64, i64)>,
) -> ApiResult<StatusCode> {
    access.require(Role::Owner)?;
    if !portfolios::unshare(&state.pool, access.0.portfolio.id, user_id).await? {
        return Err(ApiError::not_found());
    }
    let detail = serde_json::json!({ "user_id": user_id }).to_string();
    record_audit(
        &state,
        entry(&session.user, &access.0, "portfolio.unshare", Some(&detail)),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/memberships/{portfolio_id}`: the signed-in user leaves a portfolio
/// shared with them. This sits outside the portfolio routes because a viewer may
/// leave, and the guard refuses every write from a viewer.
pub async fn leave(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    PathParam(portfolio_id): PathParam<i64>,
) -> ApiResult<StatusCode> {
    let user = &session.user;
    let access = portfolios::access(&state.pool, user.id, portfolio_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if access.role == Role::Owner {
        return Err(ApiError::bad_request(
            "owner_cannot_leave",
            "the owner cannot leave their own portfolio; archive it instead",
        ));
    }
    portfolios::unshare(&state.pool, portfolio_id, user.id).await?;
    record_audit(&state, entry(user, &access, "portfolio.leave", None)).await;
    Ok(StatusCode::NO_CONTENT)
}
