//! Portfolio accounts over HTTP.

use axum::{Json, extract::State, http::StatusCode};
use saffron_core::book::AccountKind;
use saffron_db::{
    accounts::{self, Account},
    is_unique_violation,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    portfolios::{self, guard::PortfolioAccess},
    request::{JsonBody, PathParam},
    state::AppState,
    users::record_audit,
};

/// Longest institution name, in characters.
pub const MAX_INSTITUTION_CHARS: usize = 100;

/// An account as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    /// Row id.
    pub id: i64,
    /// Name, unique within the portfolio.
    pub name: String,
    /// What kind of account it is.
    pub kind: String,
    /// The ISO 4217 currency its amounts are kept in.
    pub currency: String,
    /// The bank or broker holding it, when known.
    pub institution: Option<String>,
    /// When it was created.
    pub created_at: String,
    /// When it was closed; `None` while open.
    pub closed_at: Option<String>,
}

impl From<&Account> for AccountView {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id,
            name: account.name.clone(),
            kind: account.kind.as_str().to_owned(),
            currency: account.currency.clone(),
            institution: account.institution.clone(),
            created_at: account.created_at.clone(),
            closed_at: account.closed_at.clone(),
        }
    }
}

/// Body of `POST /api/portfolios/{portfolio_id}/accounts`.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    kind: String,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    institution: Option<String>,
}

/// Body of `PATCH /api/portfolios/{portfolio_id}/accounts/{account_id}`.
#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    name: String,
}

/// `GET /api/portfolios/{portfolio_id}/accounts`
pub async fn list(
    State(state): State<AppState>,
    access: PortfolioAccess,
) -> ApiResult<Json<Vec<AccountView>>> {
    let rows = accounts::list(&state.pool, access.0.portfolio.id).await?;
    Ok(Json(rows.iter().map(AccountView::from).collect()))
}

/// `POST /api/portfolios/{portfolio_id}/accounts`
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<CreateRequest>,
) -> ApiResult<(StatusCode, Json<AccountView>)> {
    let portfolio = &access.0.portfolio;
    let name = portfolios::validate_name(&request.name, "account")?;
    let kind = AccountKind::parse(request.kind.trim()).ok_or_else(|| {
        ApiError::bad_request(
            "invalid_kind",
            "kind is one of book_entry, equity_savings, fund, savings, cash, other",
        )
    })?;
    let currency = match request.currency.as_deref() {
        Some(code) => portfolios::validate_currency(code)?,
        None => portfolio.base_currency.clone(),
    };
    let institution = request
        .institution
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if institution.is_some_and(|value| {
        value.chars().count() > MAX_INSTITUTION_CHARS || value.chars().any(char::is_control)
    }) {
        return Err(ApiError::bad_request(
            "invalid_institution",
            format!("an institution has at most {MAX_INSTITUTION_CHARS} characters"),
        ));
    }
    let new = accounts::NewAccount {
        portfolio_id: portfolio.id,
        name,
        kind,
        currency: &currency,
        institution,
    };
    let id = accounts::create(&state.pool, new, state.clock.now())
        .await
        .map_err(name_taken)?;
    record_audit(
        &state,
        portfolios::entry(&session.user, &access.0, "account.create", Some(name)),
    )
    .await;
    let account = reload(&state, portfolio.id, id).await?;
    Ok((StatusCode::CREATED, Json(account)))
}

/// `PATCH /api/portfolios/{portfolio_id}/accounts/{account_id}`: renames it.
pub async fn rename(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    PathParam((_, id)): PathParam<(i64, i64)>,
    JsonBody(request): JsonBody<RenameRequest>,
) -> ApiResult<Json<AccountView>> {
    let name = portfolios::validate_name(&request.name, "account")?;
    let account = accounts::get(&state.pool, access.0.portfolio.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    accounts::rename(&state.pool, access.0.portfolio.id, id, name)
        .await
        .map_err(name_taken)?;
    let detail = serde_json::json!({ "from": account.name, "to": name }).to_string();
    record_audit(
        &state,
        portfolios::entry(&session.user, &access.0, "account.rename", Some(&detail)),
    )
    .await;
    Ok(Json(reload(&state, access.0.portfolio.id, id).await?))
}

/// `POST /api/portfolios/{portfolio_id}/accounts/{account_id}/close`
pub async fn close(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    PathParam((_, id)): PathParam<(i64, i64)>,
) -> ApiResult<Json<AccountView>> {
    set_closed(&state, &session.user, &access, id, true).await
}

/// `POST /api/portfolios/{portfolio_id}/accounts/{account_id}/reopen`
pub async fn reopen(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    PathParam((_, id)): PathParam<(i64, i64)>,
) -> ApiResult<Json<AccountView>> {
    set_closed(&state, &session.user, &access, id, false).await
}

async fn set_closed(
    state: &AppState,
    user: &saffron_db::users::User,
    access: &PortfolioAccess,
    id: i64,
    closed: bool,
) -> ApiResult<Json<AccountView>> {
    let portfolio_id = access.0.portfolio.id;
    let _writing = state.book_writes.lock().await;
    accounts::get(&state.pool, portfolio_id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let changed = if closed {
        accounts::close(&state.pool, portfolio_id, id, state.clock.now()).await?
    } else {
        accounts::reopen(&state.pool, portfolio_id, id).await?
    };
    if changed {
        let action = if closed {
            "account.close"
        } else {
            "account.reopen"
        };
        record_audit(state, portfolios::entry(user, &access.0, action, None)).await;
    }
    Ok(Json(reload(state, portfolio_id, id).await?))
}

async fn reload(state: &AppState, portfolio_id: i64, id: i64) -> ApiResult<AccountView> {
    let account = accounts::get(&state.pool, portfolio_id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(AccountView::from(&account))
}

fn name_taken(error: sqlx::Error) -> ApiError {
    if is_unique_violation(&error) {
        ApiError::conflict(
            "name_taken",
            "this portfolio already has an account with this name",
        )
    } else {
        error.into()
    }
}
