//! What the portfolio holds: cash, realized results, and lots as the book engine
//! computes them.

use axum::{Json, extract::State};
use saffron_core::book as engine;
use saffron_db::{accounts, decimal, transactions};
use serde::{Deserialize, Serialize};

use crate::{
    error::{ApiError, ApiResult},
    portfolios::guard::PortfolioAccess,
    state::AppState,
};

use super::{engine_accounts, engine_transactions};

/// Everything the book says about one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountHoldings {
    /// The account.
    pub account_id: i64,
    /// The ISO 4217 currency its amounts are in.
    pub currency: String,
    /// Cash on the account, as decimal text.
    pub cash: String,
    /// Realized result booked so far, as decimal text.
    pub realized: String,
    /// What it holds, by instrument.
    pub holdings: Vec<HoldingView>,
}

/// The units of one instrument an account holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldingView {
    /// The instrument.
    pub instrument_id: i64,
    /// Units held, as decimal text.
    pub quantity: String,
    /// What they cost, as decimal text.
    pub cost_basis: String,
    /// Cost per unit, when anything is held.
    pub average_cost: Option<String>,
    /// The remaining lots, oldest first.
    pub lots: Vec<LotView>,
}

/// Units bought at one time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LotView {
    /// The day they were acquired.
    pub acquired: String,
    /// Units left in the lot, as decimal text.
    pub quantity: String,
    /// Cost per unit, fees included, as decimal text.
    pub unit_cost: String,
}

fn holding_view(instrument_id: i64, holding: &engine::Holding) -> HoldingView {
    HoldingView {
        instrument_id,
        quantity: decimal::to_text(holding.quantity),
        cost_basis: decimal::to_text(holding.cost_basis),
        average_cost: holding.average_cost().map(decimal::to_text),
        lots: holding
            .lots
            .iter()
            .map(|lot| LotView {
                acquired: lot.acquired.to_string(),
                quantity: decimal::to_text(lot.quantity),
                unit_cost: decimal::to_text(lot.unit_cost),
            })
            .collect(),
    }
}

/// `GET /api/portfolios/{portfolio_id}/holdings`: every account's cash, realized
/// result, and lots.
pub async fn show(
    State(state): State<AppState>,
    access: PortfolioAccess,
) -> ApiResult<Json<Vec<AccountHoldings>>> {
    let portfolio_id = access.0.portfolio.id;
    let accounts = accounts::list(&state.pool, portfolio_id).await?;
    let transactions = transactions::list(&state.pool, portfolio_id).await?;
    let book = engine::read(
        &engine_accounts(&accounts),
        &engine_transactions(&transactions),
    )
    .map_err(|error| ApiError::internal(format!("the stored book is not valid: {error}")))?;
    Ok(Json(
        book.accounts
            .into_iter()
            .map(|(account_id, state)| AccountHoldings {
                account_id,
                currency: state.currency,
                cash: decimal::to_text(state.cash),
                realized: decimal::to_text(state.realized),
                holdings: state
                    .holdings
                    .iter()
                    .map(|(instrument_id, holding)| holding_view(*instrument_id, holding))
                    .collect(),
            })
            .collect(),
    ))
}
