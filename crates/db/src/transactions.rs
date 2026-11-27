//! Transactions: the rows the book engine reads, scoped to their portfolio
//! through their account.

use jiff::{Timestamp, civil::Date};
use rust_decimal::Decimal;
use saffron_core::book::Kind;
use sqlx::SqlitePool;

use crate::{decimal, time};

/// A stored transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// Row id; also breaks ties within a day.
    pub id: i64,
    /// The account it belongs to.
    pub account_id: i64,
    /// The security, for the kinds that name one.
    pub instrument_id: Option<i64>,
    /// What it did.
    pub kind: Kind,
    /// The day it happened.
    pub date: Date,
    /// Units; for a split, the new-per-old ratio.
    pub quantity: Option<Decimal>,
    /// Price per unit in [`Self::currency`].
    pub price: Option<Decimal>,
    /// Cash amount before fees, in [`Self::currency`].
    pub amount: Option<Decimal>,
    /// The charge, in [`Self::currency`].
    pub fee: Decimal,
    /// The ISO 4217 currency of the amounts.
    pub currency: String,
    /// Account-currency units per unit of [`Self::currency`], when they differ.
    pub fx_rate: Option<Decimal>,
    /// A free-text note.
    pub note: Option<String>,
}

/// The fields of a new transaction.
#[derive(Debug, Clone, Copy)]
pub struct NewTransaction<'a> {
    /// The account it belongs to.
    pub account_id: i64,
    /// The security, for the kinds that name one.
    pub instrument_id: Option<i64>,
    /// What it did.
    pub kind: Kind,
    /// The day it happened.
    pub date: Date,
    /// Units; for a split, the new-per-old ratio.
    pub quantity: Option<Decimal>,
    /// Price per unit in [`Self::currency`].
    pub price: Option<Decimal>,
    /// Cash amount before fees, in [`Self::currency`].
    pub amount: Option<Decimal>,
    /// The charge, in [`Self::currency`].
    pub fee: Decimal,
    /// The ISO 4217 currency of the amounts.
    pub currency: &'a str,
    /// Account-currency units per unit of [`Self::currency`], when they differ.
    pub fx_rate: Option<Decimal>,
    /// A free-text note.
    pub note: Option<&'a str>,
}

type TransactionRow = (
    i64,
    i64,
    Option<i64>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
);

const LIST_SQL: &str = concat!(
    "SELECT t.id, t.account_id, t.instrument_id, t.kind, t.trade_date, t.quantity, t.price, ",
    "t.amount, t.fee, t.currency, t.fx_rate, t.note FROM transactions t ",
    "JOIN accounts a ON a.id = t.account_id ",
    "WHERE a.portfolio_id = ? ORDER BY t.trade_date, t.id"
);

const LIST_FOR_ACCOUNT_SQL: &str = concat!(
    "SELECT t.id, t.account_id, t.instrument_id, t.kind, t.trade_date, t.quantity, t.price, ",
    "t.amount, t.fee, t.currency, t.fx_rate, t.note FROM transactions t ",
    "JOIN accounts a ON a.id = t.account_id ",
    "WHERE a.portfolio_id = ? AND t.account_id = ? ORDER BY t.trade_date, t.id"
);

fn decode_optional(text: Option<String>) -> Result<Option<Decimal>, sqlx::Error> {
    text.map(|text| decimal::from_text(&text))
        .transpose()
        .map_err(|error| sqlx::Error::Decode(error.to_string().into()))
}

fn from_row(row: TransactionRow) -> Result<Transaction, sqlx::Error> {
    let (
        id,
        account_id,
        instrument_id,
        kind,
        trade_date,
        quantity,
        price,
        amount,
        fee,
        currency,
        fx_rate,
        note,
    ) = row;
    let kind = Kind::parse(&kind)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown transaction kind `{kind}`").into()))?;
    Ok(Transaction {
        id,
        account_id,
        instrument_id,
        kind,
        date: trade_date.parse().map_err(|error| {
            sqlx::Error::Decode(format!("bad trade date `{trade_date}`: {error}").into())
        })?,
        quantity: decode_optional(quantity)?,
        price: decode_optional(price)?,
        amount: decode_optional(amount)?,
        fee: decimal::from_text(&fee)
            .map_err(|error| sqlx::Error::Decode(error.to_string().into()))?,
        currency,
        fx_rate: decode_optional(fx_rate)?,
        note,
    })
}

/// Every transaction of `portfolio_id`, by day and row id.
pub async fn list(pool: &SqlitePool, portfolio_id: i64) -> Result<Vec<Transaction>, sqlx::Error> {
    let rows: Vec<TransactionRow> = sqlx::query_as(LIST_SQL)
        .bind(portfolio_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(from_row).collect()
}

/// The transactions of one account, by day and row id.
pub async fn list_for_account(
    pool: &SqlitePool,
    portfolio_id: i64,
    account_id: i64,
) -> Result<Vec<Transaction>, sqlx::Error> {
    let rows: Vec<TransactionRow> = sqlx::query_as(LIST_FOR_ACCOUNT_SQL)
        .bind(portfolio_id)
        .bind(account_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(from_row).collect()
}

/// Stores `transaction` and returns its id. The account must already belong to the
/// portfolio the request is scoped to.
pub async fn create(
    pool: &SqlitePool,
    transaction: NewTransaction<'_>,
    created_by: i64,
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    let at = time::to_text(now);
    sqlx::query_scalar(
        "INSERT INTO transactions
             (account_id, instrument_id, kind, trade_date, quantity, price, amount, fee,
              currency, fx_rate, note, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
         RETURNING id",
    )
    .bind(transaction.account_id)
    .bind(transaction.instrument_id)
    .bind(transaction.kind.as_str())
    .bind(transaction.date.to_string())
    .bind(transaction.quantity.map(decimal::to_text))
    .bind(transaction.price.map(decimal::to_text))
    .bind(transaction.amount.map(decimal::to_text))
    .bind(decimal::to_text(transaction.fee))
    .bind(transaction.currency)
    .bind(transaction.fx_rate.map(decimal::to_text))
    .bind(transaction.note)
    .bind(created_by)
    .bind(at)
    .fetch_one(pool)
    .await
}

/// Deletes transaction `id` when it is in `portfolio_id`. Returns whether a row
/// was removed.
pub async fn delete(pool: &SqlitePool, portfolio_id: i64, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM transactions WHERE id = ?1 AND account_id IN
             (SELECT id FROM accounts WHERE portfolio_id = ?2)",
    )
    .bind(id)
    .bind(portfolio_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}
