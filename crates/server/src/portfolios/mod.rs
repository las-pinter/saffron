//! Portfolios: each user owns theirs and may share one with other users as a viewer
//! or an editor. Every route about one portfolio sits behind [`guard::guard`].

pub mod guard;
pub mod shares;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{get, post, put},
};
use saffron_db::{
    audit, instruments, is_unique_violation,
    portfolios::{self, Access, NewPortfolio, Role},
    users::User,
};
use serde::{Deserialize, Serialize};

use self::guard::PortfolioAccess;
use crate::{
    auth::extract::CurrentUser,
    book,
    error::{ApiError, ApiResult},
    exports, imports, performance,
    request::JsonBody,
    state::AppState,
    targets,
    users::record_audit,
};

/// Longest portfolio name, in characters.
pub const MAX_NAME_CHARS: usize = 100;
/// Base currency when none is given.
pub const DEFAULT_CURRENCY: &str = "EUR";

/// The routes about one portfolio, nested at `/portfolios/{portfolio_id}` behind the
/// access guard.
pub fn one_portfolio(state: AppState) -> Router<AppState> {
    let routes = Router::new()
        .route("/", get(show).patch(rename))
        .route("/archive", post(archive))
        .route("/unarchive", post(unarchive))
        .route("/shares", get(shares::list).post(shares::put))
        .route("/shares/{user_id}", axum::routing::delete(shares::remove))
        .route(
            "/accounts",
            get(book::accounts::list).post(book::accounts::create),
        )
        .route(
            "/accounts/{account_id}",
            axum::routing::patch(book::accounts::rename),
        )
        .route("/accounts/{account_id}/close", post(book::accounts::close))
        .route(
            "/accounts/{account_id}/reopen",
            post(book::accounts::reopen),
        )
        .route(
            "/transactions",
            get(book::transactions::list).post(book::transactions::create),
        )
        .route(
            "/transactions/{transaction_id}",
            axum::routing::delete(book::transactions::delete),
        )
        .route("/holdings", get(book::holdings::show))
        .route("/performance", get(performance::show))
        .route("/targets", get(targets::show).put(targets::replace))
        .route("/benchmark", put(set_benchmark))
        .route("/import", post(imports::run))
        .route("/import/preview", post(imports::preview))
        .route("/export.csv", get(exports::csv))
        .route("/export.json", get(exports::json));
    guarded(routes, state)
}

/// Puts `routes` behind the access guard. Every router nested under a portfolio id
/// goes through here; tests use it to check the guard on probe routes.
pub fn guarded(routes: Router<AppState>, state: AppState) -> Router<AppState> {
    routes.route_layer(middleware::from_fn_with_state(state, guard::guard))
}

/// The owner as a portfolio shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerView {
    /// The owner's id.
    pub id: i64,
    /// The owner's sign-in name.
    pub username: String,
    /// The owner's display name.
    pub display_name: String,
}

/// A portfolio as the API shows it to one user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioView {
    /// Row id.
    pub id: i64,
    /// Name, unique per owner.
    pub name: String,
    /// ISO 4217 code that values are reported in.
    pub base_currency: String,
    /// The benchmark instrument; `None` compares against the default.
    pub benchmark_instrument_id: Option<i64>,
    /// The viewing user's role: `owner`, `editor`, or `viewer`.
    pub role: String,
    /// Who owns it.
    pub owner: OwnerView,
    /// When it was created.
    pub created_at: String,
    /// When it was archived; `None` while active.
    pub archived_at: Option<String>,
}

impl From<&Access> for PortfolioView {
    fn from(access: &Access) -> Self {
        let portfolio = &access.portfolio;
        Self {
            id: portfolio.id,
            name: portfolio.name.clone(),
            base_currency: portfolio.base_currency.clone(),
            benchmark_instrument_id: portfolio.benchmark_instrument_id,
            role: access.role.as_str().to_owned(),
            owner: OwnerView {
                id: portfolio.owner_id,
                username: access.owner_username.clone(),
                display_name: access.owner_display_name.clone(),
            },
            created_at: portfolio.created_at.clone(),
            archived_at: portfolio.archived_at.clone(),
        }
    }
}

/// Body of `POST /api/portfolios`.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    #[serde(default)]
    base_currency: Option<String>,
}

/// Body of `PATCH /api/portfolios/{portfolio_id}`.
#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    name: String,
}

/// `GET /api/portfolios`: the portfolios the user owns or has shared with them.
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
) -> ApiResult<Json<Vec<PortfolioView>>> {
    let visible = portfolios::visible_to(&state.pool, session.user.id).await?;
    Ok(Json(visible.iter().map(PortfolioView::from).collect()))
}

/// `POST /api/portfolios`: creates a portfolio owned by the user.
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<CreateRequest>,
) -> ApiResult<(StatusCode, Json<PortfolioView>)> {
    let user = &session.user;
    let name = validate_name(&request.name, "portfolio")?;
    let currency = validate_currency(request.base_currency.as_deref().unwrap_or(DEFAULT_CURRENCY))?;
    let new = NewPortfolio {
        owner_id: user.id,
        name,
        base_currency: &currency,
    };
    let id = portfolios::create(&state.pool, new, state.clock.now())
        .await
        .map_err(name_taken)?;
    let access = portfolios::access(&state.pool, user.id, id)
        .await?
        .ok_or_else(|| ApiError::internal("a new portfolio is not visible to its owner"))?;
    record_audit(&state, entry(user, &access, "portfolio.create", None)).await;
    Ok((StatusCode::CREATED, Json(PortfolioView::from(&access))))
}

/// `GET /api/portfolios/{portfolio_id}`
#[allow(clippy::unused_async)]
pub async fn show(access: PortfolioAccess) -> Json<PortfolioView> {
    Json(PortfolioView::from(&*access.0))
}

/// `PATCH /api/portfolios/{portfolio_id}`: renames it. Owner only.
pub async fn rename(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<RenameRequest>,
) -> ApiResult<Json<PortfolioView>> {
    access.require(Role::Owner)?;
    let name = validate_name(&request.name, "portfolio")?;
    let id = access.0.portfolio.id;
    portfolios::rename(&state.pool, id, name)
        .await
        .map_err(name_taken)?;
    let detail = serde_json::json!({ "from": access.0.portfolio.name, "to": name }).to_string();
    record_audit(
        &state,
        entry(&session.user, &access.0, "portfolio.rename", Some(&detail)),
    )
    .await;
    reloaded(&state, &session.user, id).await
}

/// `POST /api/portfolios/{portfolio_id}/archive`: hides it from daily use. Owner only.
pub async fn archive(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
) -> ApiResult<Json<PortfolioView>> {
    set_archived(&state, &session.user, &access, true).await
}

/// `POST /api/portfolios/{portfolio_id}/unarchive`: brings it back. Owner only.
pub async fn unarchive(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
) -> ApiResult<Json<PortfolioView>> {
    set_archived(&state, &session.user, &access, false).await
}

async fn set_archived(
    state: &AppState,
    user: &User,
    access: &PortfolioAccess,
    archived: bool,
) -> ApiResult<Json<PortfolioView>> {
    access.require(Role::Owner)?;
    let id = access.0.portfolio.id;
    let at = archived.then(|| state.clock.now());
    if !portfolios::set_archived(&state.pool, id, at).await? {
        return reloaded(state, user, id).await;
    }
    let action = if archived {
        "portfolio.archive"
    } else {
        "portfolio.unarchive"
    };
    record_audit(state, entry(user, &access.0, action, None)).await;
    reloaded(state, user, id).await
}

/// Body of `PUT /api/portfolios/{portfolio_id}/benchmark`.
#[derive(Debug, Deserialize)]
pub struct BenchmarkRequest {
    #[serde(default)]
    instrument_id: Option<i64>,
}

/// `PUT /api/portfolios/{portfolio_id}/benchmark`: compares the portfolio against
/// `instrument_id`; `null` restores the default benchmark.
pub async fn set_benchmark(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<BenchmarkRequest>,
) -> ApiResult<Json<PortfolioView>> {
    access.require(Role::Editor)?;
    if let Some(instrument_id) = request.instrument_id
        && instruments::get(&state.pool, instrument_id)
            .await?
            .is_none()
    {
        return Err(ApiError::bad_request(
            "unknown_instrument",
            "there is no instrument with that id",
        ));
    }
    let id = access.0.portfolio.id;
    if !portfolios::set_benchmark(&state.pool, id, request.instrument_id).await? {
        return Err(ApiError::not_found());
    }
    let detail = serde_json::json!({ "instrument_id": request.instrument_id }).to_string();
    record_audit(
        &state,
        entry(
            &session.user,
            &access.0,
            "portfolio.set_benchmark",
            Some(&detail),
        ),
    )
    .await;
    reloaded(&state, &session.user, id).await
}

async fn reloaded(state: &AppState, user: &User, id: i64) -> ApiResult<Json<PortfolioView>> {
    let access = portfolios::access(&state.pool, user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(PortfolioView::from(&access)))
}

pub(crate) fn validate_name<'a>(raw: &'a str, noun: &str) -> ApiResult<&'a str> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request(
            "invalid_name",
            format!("a {noun} name has 1 to {MAX_NAME_CHARS} characters"),
        ));
    }
    Ok(name)
}

/// An ISO 4217 code: three letters, stored in upper case.
pub(crate) fn validate_currency(raw: &str) -> ApiResult<String> {
    let code = raw.trim().to_ascii_uppercase();
    if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Ok(code);
    }
    Err(ApiError::bad_request(
        "invalid_currency",
        "the base currency is a three-letter ISO 4217 code, such as EUR",
    ))
}

fn name_taken(error: sqlx::Error) -> ApiError {
    if is_unique_violation(&error) {
        ApiError::conflict("name_taken", "you already have a portfolio with this name")
    } else {
        error.into()
    }
}

pub(crate) fn entry<'a>(
    user: &'a User,
    access: &'a Access,
    action: &'a str,
    detail: Option<&'a str>,
) -> audit::Entry<'a> {
    audit::Entry {
        user_id: Some(user.id),
        action,
        target: Some(&access.portfolio.name),
        detail,
    }
}
