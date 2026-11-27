//! Portfolio export: transactions as a CSV that imports back, and the whole
//! portfolio as JSON.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
};
use rust_decimal::Decimal;
use saffron_db::{accounts, decimal, instruments, transactions};
use serde::{Deserialize, Serialize};

use crate::{
    book::{
        accounts::AccountView,
        holdings::{self, AccountHoldings},
        transactions::TransactionView,
    },
    error::{ApiError, ApiResult},
    portfolios::{PortfolioView, guard::PortfolioAccess},
    state::AppState,
};

/// The CSV header row; an import maps its fields to these names.
pub const COLUMNS: [&str; 11] = [
    "account",
    "kind",
    "date",
    "instrument",
    "quantity",
    "price",
    "amount",
    "fee",
    "currency",
    "fx_rate",
    "note",
];

/// A portfolio as the JSON export holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioExport {
    /// The portfolio.
    pub portfolio: PortfolioView,
    /// Its accounts.
    pub accounts: Vec<AccountView>,
    /// Its transactions.
    pub transactions: Vec<TransactionView>,
    /// Its holdings as the book engine computes them.
    pub holdings: Vec<AccountHoldings>,
}

/// `GET /api/portfolios/{portfolio_id}/export.csv`
///
/// Cells are written as stored. A note or name that begins with a spreadsheet
/// formula character is not escaped: the file is meant for Saffron's own import,
/// and opening it in a spreadsheet is the owner's choice.
pub async fn csv(
    State(state): State<AppState>,
    access: PortfolioAccess,
) -> ApiResult<(StatusCode, HeaderMap, Vec<u8>)> {
    let portfolio_id = access.0.portfolio.id;
    // A write between the account and transaction reads could break the book view.
    let _reading = state.book_writes.lock().await;
    let accounts = accounts::list(&state.pool, portfolio_id).await?;
    let names: BTreeMap<i64, String> = accounts
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();
    let symbols: BTreeMap<i64, String> = instruments::summaries(&state.pool)
        .await?
        .into_iter()
        .filter_map(|summary| summary.symbol.map(|symbol| (summary.id, symbol)))
        .collect();
    let rows = transactions::list(&state.pool, portfolio_id).await?;
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(COLUMNS).map_err(write_error)?;
    for row in &rows {
        let text = |value: Option<Decimal>| value.map(decimal::to_text).unwrap_or_default();
        let instrument = row
            .instrument_id
            .and_then(|id| symbols.get(&id).cloned())
            .unwrap_or_default();
        let account = names.get(&row.account_id).cloned().unwrap_or_default();
        let record = [
            account,
            row.kind.as_str().to_owned(),
            row.date.to_string(),
            instrument,
            text(row.quantity),
            text(row.price),
            text(row.amount),
            decimal::to_text(row.fee),
            row.currency.clone(),
            text(row.fx_rate),
            row.note.clone().unwrap_or_default(),
        ];
        writer.write_record(record).map_err(write_error)?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|error| write_error(error.into_error()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    let disposition = format!("attachment; filename=\"portfolio-{portfolio_id}-transactions.csv\"");
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .map_err(|_| ApiError::internal("the CSV file name is not a header value"))?,
    );
    Ok((StatusCode::OK, headers, bytes))
}

/// `GET /api/portfolios/{portfolio_id}/export.json`
pub async fn json(
    State(state): State<AppState>,
    access: PortfolioAccess,
) -> ApiResult<Json<PortfolioExport>> {
    let portfolio_id = access.0.portfolio.id;
    let _reading = state.book_writes.lock().await;
    let accounts = accounts::list(&state.pool, portfolio_id).await?;
    let transactions = transactions::list(&state.pool, portfolio_id).await?;
    let holdings = holdings::build(&accounts, &transactions)?;
    Ok(Json(PortfolioExport {
        portfolio: PortfolioView::from(&*access.0),
        accounts: accounts.iter().map(AccountView::from).collect(),
        transactions: transactions.iter().map(TransactionView::stored).collect(),
        holdings,
    }))
}

fn write_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("the CSV cannot be written: {error}"))
}
