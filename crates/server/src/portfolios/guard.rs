//! The access check in front of every route under `/api/portfolios/{portfolio_id}`.
//!
//! The check is a layer on that whole subtree, so a route added there later cannot
//! skip it. A user who neither owns the portfolio nor has it shared with them gets
//! 404, as if it did not exist; a viewer gets 403 on any method that writes. An
//! archived portfolio is read-only: every write gets 409 except archiving and
//! restoring it.
//! Handlers read the outcome through [`PortfolioAccess`], and owner-only actions
//! ask for [`Role::Owner`] themselves.

use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, MatchedPath, RawPathParams, Request, State},
    http::{Method, request::Parts},
    middleware::Next,
    response::Response,
};
use saffron_db::portfolios::{self, Access, Role};

use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    state::AppState,
};

/// The path parameter that names the portfolio.
pub const PARAM: &str = "portfolio_id";
/// Route endings that stay open on an archived portfolio: restoring it, and
/// previewing an import, which writes nothing.
const ARCHIVE_ALLOWED: [&str; 3] = ["/archive", "/unarchive", "/import/preview"];

/// The signed-in user's access to the portfolio in the path, set by [`guard`].
#[derive(Debug, Clone)]
pub struct PortfolioAccess(pub Arc<Access>);

impl PortfolioAccess {
    /// Refuses with 403 unless the user holds at least `needed`.
    pub fn require(&self, needed: Role) -> ApiResult<()> {
        if self.0.role >= needed {
            return Ok(());
        }
        Err(ApiError::forbidden(
            "insufficient_role",
            format!("this needs the {} role on the portfolio", needed.as_str()),
        ))
    }
}

impl FromRequestParts<AppState> for PortfolioAccess {
    type Rejection = ApiError;

    fn from_request_parts(
        parts: &mut Parts,
        _: &AppState,
    ) -> impl Future<Output = Result<Self, ApiError>> + Send {
        std::future::ready(
            parts
                .extensions
                .get::<Self>()
                .cloned()
                .ok_or_else(|| ApiError::internal("a portfolio route is missing its access guard")),
        )
    }
}

/// Resolves the user's role on the portfolio in the path before any handler runs.
pub async fn guard(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    params: RawPathParams,
    mut request: Request,
    next: Next,
) -> ApiResult<Response> {
    let id = params
        .iter()
        .find(|(name, _)| *name == PARAM)
        .and_then(|(_, value)| value.parse::<i64>().ok())
        .ok_or_else(|| ApiError::bad_request("invalid_path", "the portfolio id is not a number"))?;
    let access = portfolios::access(&state.pool, session.user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let access = PortfolioAccess(Arc::new(access));
    if !is_read(request.method()) {
        access.require(Role::Editor)?;
        if access.0.portfolio.archived_at.is_some() && !allows_archived_write(&request) {
            return Err(ApiError::conflict(
                "archived",
                "the portfolio is archived; restore it to change it",
            ));
        }
    }
    request.extensions_mut().insert(access);
    Ok(next.run(request).await)
}

fn allows_archived_write(request: &Request) -> bool {
    request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|path| {
            ARCHIVE_ALLOWED
                .iter()
                .any(|end| path.as_str().ends_with(end))
        })
}

fn is_read(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}
