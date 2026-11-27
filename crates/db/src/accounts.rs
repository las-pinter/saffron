//! Portfolio accounts: the containers that transactions and cash belong to.

use jiff::Timestamp;
use saffron_core::book::AccountKind;
use sqlx::SqlitePool;

use crate::time;

/// A stored account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Row id.
    pub id: i64,
    /// The portfolio it belongs to.
    pub portfolio_id: i64,
    /// Name, unique within the portfolio.
    pub name: String,
    /// What kind of account it is.
    pub kind: AccountKind,
    /// The ISO 4217 currency its amounts are kept in.
    pub currency: String,
    /// The bank or broker holding it, when known.
    pub institution: Option<String>,
    /// When it was created, as RFC 3339 text.
    pub created_at: String,
    /// When it was closed, as RFC 3339 text; `None` while open.
    pub closed_at: Option<String>,
}

/// The fields of a new account.
#[derive(Debug, Clone, Copy)]
pub struct NewAccount<'a> {
    /// The portfolio it belongs to.
    pub portfolio_id: i64,
    /// Its name.
    pub name: &'a str,
    /// What kind of account it is.
    pub kind: AccountKind,
    /// The ISO 4217 currency its amounts are kept in.
    pub currency: &'a str,
    /// The bank or broker holding it, when known.
    pub institution: Option<&'a str>,
}

type AccountRow = (
    i64,
    i64,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
);

fn from_row(row: AccountRow) -> Result<Account, sqlx::Error> {
    let (id, portfolio_id, name, kind, currency, institution, created_at, closed_at) = row;
    let kind = AccountKind::parse(&kind)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown account kind `{kind}`").into()))?;
    Ok(Account {
        id,
        portfolio_id,
        name,
        kind,
        currency,
        institution,
        created_at,
        closed_at,
    })
}

/// The accounts of `portfolio_id`, open and closed, by name.
pub async fn list(pool: &SqlitePool, portfolio_id: i64) -> Result<Vec<Account>, sqlx::Error> {
    let rows: Vec<AccountRow> = sqlx::query_as(
        "SELECT id, portfolio_id, name, kind, currency, institution, created_at, closed_at
         FROM accounts WHERE portfolio_id = ? ORDER BY name, id",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// Account `id`, when it belongs to `portfolio_id`.
pub async fn get(
    pool: &SqlitePool,
    portfolio_id: i64,
    id: i64,
) -> Result<Option<Account>, sqlx::Error> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, portfolio_id, name, kind, currency, institution, created_at, closed_at
         FROM accounts WHERE id = ? AND portfolio_id = ?",
    )
    .bind(id)
    .bind(portfolio_id)
    .fetch_optional(pool)
    .await?;
    row.map(from_row).transpose()
}

/// Creates an account and returns its id. A name already used in the portfolio
/// fails with a unique-constraint error; see [`crate::is_unique_violation`].
pub async fn create(
    pool: &SqlitePool,
    account: NewAccount<'_>,
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO accounts (portfolio_id, name, kind, currency, institution, created_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(account.portfolio_id)
    .bind(account.name)
    .bind(account.kind.as_str())
    .bind(account.currency)
    .bind(account.institution)
    .bind(time::to_text(now))
    .fetch_one(pool)
    .await
}

/// Renames the account. Returns `false` when it is not in the portfolio.
pub async fn rename(
    pool: &SqlitePool,
    portfolio_id: i64,
    id: i64,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE accounts SET name = ? WHERE id = ? AND portfolio_id = ?")
        .bind(name)
        .bind(id)
        .bind(portfolio_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Closes the account so nothing new is recorded against it. Returns `false` when
/// it is not open or not in the portfolio.
pub async fn close(
    pool: &SqlitePool,
    portfolio_id: i64,
    id: i64,
    now: Timestamp,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE accounts SET closed_at = ? WHERE id = ? AND portfolio_id = ? AND closed_at IS NULL",
    )
    .bind(time::to_text(now))
    .bind(id)
    .bind(portfolio_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Reopens a closed account. Returns `false` when it is not closed or not in the
/// portfolio.
pub async fn reopen(pool: &SqlitePool, portfolio_id: i64, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE accounts SET closed_at = NULL WHERE id = ? AND portfolio_id = ? AND closed_at IS NOT NULL",
    )
    .bind(id)
    .bind(portfolio_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}
