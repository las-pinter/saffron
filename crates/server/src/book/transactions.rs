//! Portfolio transactions over HTTP. Every write is checked by the book engine
//! before it is stored, so the stored rows always form a valid book.

use axum::{Json, extract::State, http::StatusCode};
use jiff::civil::Date;
use rust_decimal::Decimal;
use saffron_core::book as engine;
use saffron_db::{
    accounts, decimal, instruments,
    transactions::{self, NewTransaction, Transaction},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    portfolios::{self, guard::PortfolioAccess},
    request::{JsonBody, PathParam, QueryParam},
    state::AppState,
    users::record_audit,
};

use super::{engine_accounts, engine_transactions};

/// The id the engine sees for a transaction that is not stored yet. It sorts after
/// every stored row, as the new row will once inserted.
const CANDIDATE: i64 = i64::MAX;
/// Longest note, in characters.
pub const MAX_NOTE_CHARS: usize = 1000;

/// A transaction as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionView {
    /// Row id.
    pub id: i64,
    /// The account it belongs to.
    pub account_id: i64,
    /// The security, for the kinds that name one.
    pub instrument_id: Option<i64>,
    /// What it did.
    pub kind: String,
    /// The day it happened.
    pub date: String,
    /// Units; for a split, the new-per-old ratio.
    pub quantity: Option<String>,
    /// Price per unit in `currency`.
    pub price: Option<String>,
    /// Cash amount before fees, in `currency`.
    pub amount: Option<String>,
    /// The charge, in `currency`.
    pub fee: String,
    /// The ISO 4217 currency of the amounts.
    pub currency: String,
    /// Account-currency units per unit of `currency`, when they differ.
    pub fx_rate: Option<String>,
    /// A free-text note.
    pub note: Option<String>,
}

fn text(value: Option<Decimal>) -> Option<String> {
    value.map(decimal::to_text)
}

impl TransactionView {
    pub(crate) fn new(id: i64, transaction: &engine::Transaction) -> Self {
        Self {
            id,
            account_id: transaction.account_id,
            instrument_id: transaction.instrument_id,
            kind: transaction.kind.as_str().to_owned(),
            date: transaction.date.to_string(),
            quantity: text(transaction.quantity),
            price: text(transaction.price),
            amount: text(transaction.amount),
            fee: decimal::to_text(transaction.fee),
            currency: transaction.currency.clone(),
            fx_rate: text(transaction.fx_rate),
            note: None,
        }
    }

    pub(crate) fn stored(transaction: &Transaction) -> Self {
        Self {
            id: transaction.id,
            account_id: transaction.account_id,
            instrument_id: transaction.instrument_id,
            kind: transaction.kind.as_str().to_owned(),
            date: transaction.date.to_string(),
            quantity: text(transaction.quantity),
            price: text(transaction.price),
            amount: text(transaction.amount),
            fee: decimal::to_text(transaction.fee),
            currency: transaction.currency.clone(),
            fx_rate: text(transaction.fx_rate),
            note: transaction.note.clone(),
        }
    }
}

/// Query of `GET /api/portfolios/{portfolio_id}/transactions`.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    account_id: Option<i64>,
}

/// Body of `POST /api/portfolios/{portfolio_id}/transactions`.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    account_id: i64,
    #[serde(default)]
    instrument_id: Option<i64>,
    kind: String,
    date: String,
    #[serde(default)]
    quantity: Option<String>,
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    amount: Option<String>,
    #[serde(default)]
    fee: Option<String>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    fx_rate: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

/// `GET /api/portfolios/{portfolio_id}/transactions[?account_id=…]`
pub async fn list(
    State(state): State<AppState>,
    access: PortfolioAccess,
    QueryParam(query): QueryParam<ListQuery>,
) -> ApiResult<Json<Vec<TransactionView>>> {
    let portfolio_id = access.0.portfolio.id;
    let rows = match query.account_id {
        Some(account_id) => {
            transactions::list_for_account(&state.pool, portfolio_id, account_id).await?
        }
        None => transactions::list(&state.pool, portfolio_id).await?,
    };
    Ok(Json(rows.iter().map(TransactionView::stored).collect()))
}

/// `POST /api/portfolios/{portfolio_id}/transactions`
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<CreateRequest>,
) -> ApiResult<(StatusCode, Json<TransactionView>)> {
    let portfolio_id = access.0.portfolio.id;
    let account = accounts::get(&state.pool, portfolio_id, request.account_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if account.closed_at.is_some() {
        return Err(ApiError::conflict(
            "account_closed",
            "the account is closed; reopen it to record transactions",
        ));
    }
    let kind = engine::Kind::parse(request.kind.trim())
        .ok_or_else(|| ApiError::bad_request("invalid_kind", "unknown transaction kind"))?;
    if let Some(instrument_id) = request.instrument_id
        && instruments::get(&state.pool, instrument_id)
            .await?
            .is_none()
    {
        return Err(ApiError::bad_request(
            "unknown_instrument",
            "the instrument does not exist",
        ));
    }
    let date: Date = request
        .date
        .trim()
        .parse()
        .map_err(|_| ApiError::bad_request("invalid_date", "date is YYYY-MM-DD"))?;
    let quantity = decimal_of("quantity", request.quantity.as_deref())?;
    let price = decimal_of("price", request.price.as_deref())?;
    let amount = decimal_of("amount", request.amount.as_deref())?;
    let fee = decimal_of("fee", request.fee.as_deref())?.unwrap_or(Decimal::ZERO);
    let fx_rate = decimal_of("fx_rate", request.fx_rate.as_deref())?;
    let currency = match request.currency.as_deref() {
        Some(code) => portfolios::validate_currency(code)?,
        None => account.currency.clone(),
    };
    let note = note_of(request.note.as_deref())?;
    let candidate = engine::Transaction {
        id: CANDIDATE,
        account_id: account.id,
        instrument_id: request.instrument_id,
        kind,
        date,
        quantity,
        price,
        amount,
        fee,
        currency: currency.clone(),
        fx_rate,
    };
    let new = NewTransaction {
        account_id: account.id,
        instrument_id: request.instrument_id,
        kind,
        date,
        quantity,
        price,
        amount,
        fee,
        currency: &currency,
        fx_rate,
        note,
    };
    let _writing = state.book_writes.lock().await;
    check_book(&state, portfolio_id, &candidate).await?;
    let id = transactions::create(&state.pool, new, session.user.id, state.clock.now()).await?;
    let detail = serde_json::json!({
        "account_id": account.id,
        "kind": kind.as_str(),
        "date": date.to_string(),
    })
    .to_string();
    record_audit(
        &state,
        portfolios::entry(
            &session.user,
            &access.0,
            "transaction.create",
            Some(&detail),
        ),
    )
    .await;
    let mut view = TransactionView::new(id, &candidate);
    view.note = note.map(str::to_owned);
    Ok((StatusCode::CREATED, Json(view)))
}

/// `DELETE /api/portfolios/{portfolio_id}/transactions/{transaction_id}`
pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    PathParam((_, transaction_id)): PathParam<(i64, i64)>,
) -> ApiResult<StatusCode> {
    let portfolio_id = access.0.portfolio.id;
    let _writing = state.book_writes.lock().await;
    let stored = transactions::list(&state.pool, portfolio_id).await?;
    if !stored.iter().any(|row| row.id == transaction_id) {
        return Err(ApiError::not_found());
    }
    let accounts = accounts::list(&state.pool, portfolio_id).await?;
    let states = engine_accounts(&accounts);
    let all = engine_transactions(&stored);
    engine::read(&states, &all)
        .map_err(|error| ApiError::internal(format!("the stored book is not valid: {error}")))?;
    let remaining: Vec<engine::Transaction> = all
        .into_iter()
        .filter(|row| row.id != transaction_id)
        .collect();
    if let Err(error) = engine::read(&states, &remaining) {
        return Err(ApiError::conflict(
            "would_break_book",
            format!("removing it would make the book invalid: {error}"),
        ));
    }
    transactions::delete(&state.pool, portfolio_id, transaction_id).await?;
    record_audit(
        &state,
        portfolios::entry(
            &session.user,
            &access.0,
            "transaction.delete",
            Some(&transaction_id.to_string()),
        ),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Reads the whole portfolio with `candidate` added and refuses it when the result
/// is not a valid book.
async fn check_book(
    state: &AppState,
    portfolio_id: i64,
    candidate: &engine::Transaction,
) -> ApiResult<()> {
    let accounts = accounts::list(&state.pool, portfolio_id).await?;
    let stored = transactions::list(&state.pool, portfolio_id).await?;
    let states = engine_accounts(&accounts);
    let existing = engine_transactions(&stored);
    engine::read(&states, &existing)
        .map_err(|error| ApiError::internal(format!("the stored book is not valid: {error}")))?;
    let mut all = existing;
    all.push(candidate.clone());
    engine::read(&states, &all).map_err(book_error)?;
    Ok(())
}

/// Turns a refusal of the candidate transaction into a client error. A stored row
/// can only fail after the candidate is added, because the stored book was read
/// without it first; adding a backdated row can reorder it out of validity, which
/// is the client's conflict, not a corrupt book.
fn book_error(error: engine::BookError) -> ApiError {
    match error {
        engine::BookError::Oversold {
            transaction,
            instrument,
            wanted,
            held,
        } if transaction == CANDIDATE => ApiError::conflict(
            "oversold",
            format!(
                "the sale asks for {wanted} units of instrument {instrument}, but only {held} are held"
            ),
        ),
        engine::BookError::TransferCurrency { transaction } if transaction == CANDIDATE => {
            ApiError::bad_request("transfer_currency", "a transfer must stay in one currency")
        }
        engine::BookError::Invalid {
            transaction,
            reason,
        } if transaction == CANDIDATE => ApiError::bad_request("invalid_transaction", reason),
        other => ApiError::conflict(
            "would_break_book",
            format!("the new transaction would make a stored one invalid: {other}"),
        ),
    }
}

fn decimal_of(name: &str, text: Option<&str>) -> ApiResult<Option<Decimal>> {
    let Some(text) = text else {
        return Ok(None);
    };
    decimal::from_text(text.trim())
        .map(Some)
        .map_err(|_| ApiError::bad_request("invalid_decimal", format!("{name} is not a decimal")))
}

fn note_of(raw: Option<&str>) -> ApiResult<Option<&str>> {
    let Some(note) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(ApiError::bad_request(
            "invalid_note",
            format!("a note has at most {MAX_NOTE_CHARS} characters"),
        ));
    }
    Ok(Some(note))
}
