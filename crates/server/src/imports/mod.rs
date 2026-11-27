//! CSV import of transactions: a column mapping, a preview of every row, duplicate
//! detection, and an all-or-nothing write.
//!
//! A row is a duplicate of a stored one when the account, kind, date, instrument,
//! quantity, price, amount, fee, currency, and `fx_rate` all match; the note is
//! ignored. A row repeated inside one file is imported once.
//!
//! An import needs the accounts and instruments it names to exist already: the CSV
//! carries transactions only. The export's own headers are the ones its importer
//! reads back. A closed account takes no new rows, but its stored rows still count
//! as duplicates, so re-importing a portfolio's export stays idempotent.

mod parse;

use std::collections::HashSet;

use axum::{Json, extract::State};
use saffron_core::book as engine;
use saffron_db::{
    accounts,
    transactions::{self, NewTransaction},
};
use serde::{Deserialize, Serialize};

use self::parse::{DuplicateKey, Row, line_of, parse};
use crate::{
    auth::extract::CurrentUser,
    book::{engine_accounts, engine_transactions, transactions::TransactionView},
    error::{ApiError, ApiResult},
    portfolios::{self, guard::PortfolioAccess},
    request::JsonBody,
    state::AppState,
    users::record_audit,
};

/// Largest CSV accepted, in bytes. It stays under the request body limit.
pub const MAX_CSV_BYTES: usize = 1024 * 1024;
/// Largest number of data rows accepted.
pub const MAX_ROWS: usize = 10_000;
/// The engine id of the first imported row; stored ids are far below it.
const IMPORT_CANDIDATE_BASE: i64 = i64::MAX - 10_000;

/// Which CSV column holds each field; only the first three are required.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Columns {
    account: Option<String>,
    kind: Option<String>,
    date: Option<String>,
    instrument: Option<String>,
    quantity: Option<String>,
    price: Option<String>,
    amount: Option<String>,
    fee: Option<String>,
    currency: Option<String>,
    fx_rate: Option<String>,
    note: Option<String>,
}

/// Body of a preview or import request.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    csv: String,
    columns: Columns,
}

/// One row as the preview shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewRow {
    /// The data row, counting from one.
    pub row: usize,
    /// `ok`, `duplicate`, or `error`.
    pub status: String,
    /// Why the row cannot be imported.
    pub errors: Vec<String>,
    /// The transaction the row would store, when it parses.
    #[serde(default)]
    pub transaction: Option<TransactionView>,
}

/// The answer to a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    /// Data rows in the file.
    pub rows: usize,
    /// Rows that would be stored.
    pub importable: usize,
    /// Rows already stored, or repeated in the file.
    pub duplicates: usize,
    /// Rows that cannot be imported.
    pub invalid: usize,
    /// Every row's outcome, in file order.
    pub detail: Vec<PreviewRow>,
}

/// The answer to an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Imported {
    /// Rows stored.
    pub imported: usize,
    /// Rows already stored, or repeated in the file.
    pub duplicates: usize,
}

/// `POST /api/portfolios/{portfolio_id}/import/preview`
pub async fn preview(
    State(state): State<AppState>,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<ImportRequest>,
) -> ApiResult<Json<Preview>> {
    let portfolio_id = access.0.portfolio.id;
    let outcomes = parse(&state, portfolio_id, &request).await?;
    let stored = transactions::list(&state.pool, portfolio_id).await?;
    let mut seen = stored_keys(&stored);
    let detail: Vec<PreviewRow> = outcomes
        .into_iter()
        .map(|outcome| match outcome {
            Ok(row) => preview_row(&row, &mut seen),
            Err((line, error)) => PreviewRow {
                row: line,
                status: "error".to_owned(),
                errors: vec![error],
                transaction: None,
            },
        })
        .collect();
    Ok(Json(summarize(detail)))
}

/// One parsed row's preview: a duplicate, a closed account, or importable.
fn preview_row(row: &Row, seen: &mut HashSet<DuplicateKey>) -> PreviewRow {
    let view = || {
        let mut view = TransactionView::new(0, &row.transaction);
        view.note.clone_from(&row.note);
        Some(view)
    };
    if !seen.insert(row.key.clone()) {
        return PreviewRow {
            row: row.line,
            status: "duplicate".to_owned(),
            errors: Vec::new(),
            transaction: view(),
        };
    }
    if row.closed {
        return PreviewRow {
            row: row.line,
            status: "error".to_owned(),
            errors: vec![format!("account `{}` is closed", row.account)],
            transaction: view(),
        };
    }
    PreviewRow {
        row: row.line,
        status: "ok".to_owned(),
        errors: Vec::new(),
        transaction: view(),
    }
}

/// `POST /api/portfolios/{portfolio_id}/import`
pub async fn run(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<ImportRequest>,
) -> ApiResult<Json<Imported>> {
    let portfolio_id = access.0.portfolio.id;
    let outcomes = parse(&state, portfolio_id, &request).await?;
    if let Some((line, error)) = outcomes.iter().find_map(|outcome| match outcome {
        Ok(_) => None,
        Err((line, error)) => Some((*line, error.clone())),
    }) {
        return Err(ApiError::bad_request(
            "import_invalid",
            format!("data row {line}: {error}; nothing was imported"),
        ));
    }
    let rows: Vec<Row> = outcomes.into_iter().flatten().collect();
    let _writing = state.book_writes.lock().await;
    let accounts = accounts::list(&state.pool, portfolio_id).await?;
    let stored = transactions::list(&state.pool, portfolio_id).await?;
    let mut seen = stored_keys(&stored);
    let mut to_import = Vec::new();
    let mut duplicates = 0;
    for row in rows {
        if seen.insert(row.key.clone()) {
            to_import.push(row);
        } else {
            duplicates += 1;
        }
    }
    if let Some(row) = to_import.iter().find(|row| row.closed) {
        return Err(ApiError::bad_request(
            "import_invalid",
            format!(
                "data row {}: account `{}` is closed; nothing was imported",
                row.line, row.account
            ),
        ));
    }
    if to_import.is_empty() {
        return Ok(Json(Imported {
            imported: 0,
            duplicates,
        }));
    }
    check_book(&accounts, &stored, &to_import)?;
    let imported = store(&state, &to_import, session.user.id).await?;
    let detail = serde_json::json!({
        "imported": imported,
        "duplicates": duplicates,
    })
    .to_string();
    record_audit(
        &state,
        portfolios::entry(&session.user, &access.0, "portfolio.import", Some(&detail)),
    )
    .await;
    Ok(Json(Imported {
        imported,
        duplicates,
    }))
}

/// Writes the rows; the caller has checked them and holds the book lock.
async fn store(state: &AppState, rows: &[Row], user_id: i64) -> ApiResult<usize> {
    let news: Vec<NewTransaction> = rows.iter().map(new_transaction).collect();
    let ids = transactions::create_all(&state.pool, &news, user_id, state.clock.now()).await?;
    Ok(ids.len())
}

fn summarize(detail: Vec<PreviewRow>) -> Preview {
    let invalid = detail.iter().filter(|row| row.status == "error").count();
    let duplicates = detail
        .iter()
        .filter(|row| row.status == "duplicate")
        .count();
    Preview {
        rows: detail.len(),
        importable: detail.len() - invalid - duplicates,
        duplicates,
        invalid,
        detail,
    }
}

fn stored_keys(stored: &[transactions::Transaction]) -> HashSet<DuplicateKey> {
    engine_transactions(stored)
        .iter()
        .map(DuplicateKey::of)
        .collect()
}

fn new_transaction(row: &Row) -> NewTransaction<'_> {
    let transaction = &row.transaction;
    NewTransaction {
        account_id: transaction.account_id,
        instrument_id: transaction.instrument_id,
        kind: transaction.kind,
        date: transaction.date,
        quantity: transaction.quantity,
        price: transaction.price,
        amount: transaction.amount,
        fee: transaction.fee,
        currency: &transaction.currency,
        fx_rate: transaction.fx_rate,
        note: row.note.as_deref(),
    }
}

/// Reads the whole book with every imported row added and refuses an invalid result.
fn check_book(
    accounts: &[accounts::Account],
    stored: &[transactions::Transaction],
    rows: &[Row],
) -> ApiResult<()> {
    let states = engine_accounts(accounts);
    let existing = engine_transactions(stored);
    engine::read(&states, &existing)
        .map_err(|error| ApiError::internal(format!("the stored book is not valid: {error}")))?;
    let mut all = existing;
    all.extend(rows.iter().enumerate().map(|(index, row)| {
        let mut transaction = row.transaction.clone();
        transaction.id = IMPORT_CANDIDATE_BASE + i64::try_from(index).unwrap_or_default();
        transaction
    }));
    engine::read(&states, &all).map_err(|error| match error {
        engine::BookError::Invalid {
            transaction,
            reason,
        } if transaction >= IMPORT_CANDIDATE_BASE => ApiError::bad_request(
            "import_invalid",
            format!(
                "data row {}: {reason}; nothing was imported",
                line_of(rows, transaction)
            ),
        ),
        engine::BookError::Oversold { transaction, .. } if transaction >= IMPORT_CANDIDATE_BASE => {
            ApiError::bad_request(
                "import_invalid",
                format!(
                    "data row {}: the sale asks for more units than are held; nothing was imported",
                    line_of(rows, transaction)
                ),
            )
        }
        engine::BookError::TransferCurrency { transaction }
            if transaction >= IMPORT_CANDIDATE_BASE =>
        {
            ApiError::bad_request(
                "import_invalid",
                format!(
                    "data row {}: a transfer must stay in one currency; nothing was imported",
                    line_of(rows, transaction)
                ),
            )
        }
        engine::BookError::Oversold { .. } | engine::BookError::TransferCurrency { .. } => {
            ApiError::conflict(
                "would_break_book",
                "the imported rows would make a stored transaction invalid",
            )
        }
        other => ApiError::conflict(
            "would_break_book",
            format!("the imported rows would make the book invalid: {other}"),
        ),
    })?;
    Ok(())
}
