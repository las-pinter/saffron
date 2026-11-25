//! The schema enforces what the application relies on: types, references, and rules.

use std::str::FromStr;

use rust_decimal::Decimal;
use saffron_db::{decimal, open, open_in_memory};
use sqlx::SqlitePool;

const NOW: &str = "2026-11-25T12:00:00Z";

async fn db() -> SqlitePool {
    open_in_memory().await.expect("in-memory database")
}

async fn insert_user(pool: &SqlitePool, username: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO users (username, display_name, created_at) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(username)
    .bind(username)
    .bind(NOW)
    .fetch_one(pool)
    .await
    .expect("insert user")
}

async fn insert_portfolio(pool: &SqlitePool, owner_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO portfolios (owner_id, name, base_currency, created_at)
         VALUES (?, 'Main', 'EUR', ?) RETURNING id",
    )
    .bind(owner_id)
    .bind(NOW)
    .fetch_one(pool)
    .await
}

async fn insert_account(
    pool: &SqlitePool,
    portfolio_id: i64,
    kind: &str,
    currency: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO accounts (portfolio_id, name, kind, currency, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(portfolio_id)
    .bind(format!("{kind} account"))
    .bind(kind)
    .bind(currency)
    .bind(NOW)
    .fetch_one(pool)
    .await
}

async fn insert_instrument(pool: &SqlitePool, isin: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO instruments (kind, name, isin, currency, created_at)
         VALUES ('stock', 'Nokia', ?, 'EUR', ?) RETURNING id",
    )
    .bind(isin)
    .bind(NOW)
    .fetch_one(pool)
    .await
    .expect("insert instrument")
}

async fn insert_transaction(
    pool: &SqlitePool,
    account_id: i64,
    instrument_id: Option<i64>,
    kind: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO transactions
             (account_id, instrument_id, kind, trade_date, amount, currency, created_at, updated_at)
         VALUES (?, ?, ?, '2026-11-20', '100', 'EUR', ?, ?) RETURNING id",
    )
    .bind(account_id)
    .bind(instrument_id)
    .bind(kind)
    .bind(NOW)
    .bind(NOW)
    .fetch_one(pool)
    .await
}

/// A portfolio with one book-entry account, ready for transactions.
async fn account_fixture(pool: &SqlitePool) -> i64 {
    let user = insert_user(pool, "owner").await;
    let portfolio = insert_portfolio(pool, user)
        .await
        .expect("insert portfolio");
    insert_account(pool, portfolio, "book_entry", "EUR")
        .await
        .expect("insert account")
}

fn is_constraint_violation(result: &Result<i64, sqlx::Error>) -> bool {
    matches!(result, Err(sqlx::Error::Database(error)) if error.message().contains("constraint"))
}

#[tokio::test]
async fn migrations_create_every_table() {
    let pool = db().await;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx_%'
         ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .expect("read the schema");

    assert_eq!(
        tables,
        [
            "accounts",
            "audit_log",
            "fx_rates",
            "instrument_listings",
            "instruments",
            "passkeys",
            "portfolio_shares",
            "portfolios",
            "prices",
            "sessions",
            "settings",
            "transactions",
            "user_settings",
            "users",
        ]
    );
}

#[tokio::test]
async fn foreign_keys_are_enforced() {
    let pool = db().await;

    let result = insert_portfolio(&pool, 999).await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn deleting_a_user_deletes_their_portfolios() {
    let pool = db().await;
    let user = insert_user(&pool, "leaving").await;
    insert_portfolio(&pool, user)
        .await
        .expect("insert portfolio");

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .expect("delete user");

    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM portfolios")
        .fetch_one(&pool)
        .await
        .expect("count portfolios");
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn usernames_are_unique_ignoring_case() {
    let pool = db().await;
    insert_user(&pool, "Las").await;

    let result: Result<i64, _> = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, created_at) VALUES ('las', 'x', ?) RETURNING id",
    )
    .bind(NOW)
    .fetch_one(&pool)
    .await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn account_kind_must_be_known() {
    let pool = db().await;
    let user = insert_user(&pool, "owner").await;
    let portfolio = insert_portfolio(&pool, user)
        .await
        .expect("insert portfolio");

    let result = insert_account(&pool, portfolio, "crypto_wallet", "EUR").await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn currency_must_be_an_upper_case_code() {
    let pool = db().await;
    let user = insert_user(&pool, "owner").await;
    let portfolio = insert_portfolio(&pool, user)
        .await
        .expect("insert portfolio");

    let result = insert_account(&pool, portfolio, "savings", "eur").await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn strict_tables_reject_a_blob_in_a_text_column() {
    let pool = db().await;

    // display_name has no CHECK, so only STRICT can refuse the blob.
    let result: Result<i64, _> = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, created_at)
         VALUES ('typed', X'00', ?) RETURNING id",
    )
    .bind(NOW)
    .fetch_one(&pool)
    .await;

    assert!(
        matches!(&result, Err(sqlx::Error::Database(error)) if error.message().contains("cannot store")),
        "result: {result:?}"
    );
}

#[tokio::test]
async fn buy_requires_an_instrument() {
    let pool = db().await;
    let account = account_fixture(&pool).await;

    let result = insert_transaction(&pool, account, None, "buy").await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn deposit_must_not_reference_an_instrument() {
    let pool = db().await;
    let account = account_fixture(&pool).await;
    let instrument = insert_instrument(&pool, "FI0009000681").await;

    let result = insert_transaction(&pool, account, Some(instrument), "deposit").await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn buy_with_an_instrument_is_accepted() {
    let pool = db().await;
    let account = account_fixture(&pool).await;
    let instrument = insert_instrument(&pool, "FI0009000681").await;

    let result = insert_transaction(&pool, account, Some(instrument), "buy").await;

    assert!(result.is_ok(), "result: {result:?}");
}

#[tokio::test]
async fn an_instrument_with_transactions_cannot_be_deleted() {
    let pool = db().await;
    let account = account_fixture(&pool).await;
    let instrument = insert_instrument(&pool, "FI0009000681").await;
    insert_transaction(&pool, account, Some(instrument), "buy")
        .await
        .expect("insert buy");

    let result = sqlx::query("DELETE FROM instruments WHERE id = ?")
        .bind(instrument)
        .execute(&pool)
        .await;

    assert!(
        matches!(&result, Err(sqlx::Error::Database(error)) if error.message().contains("FOREIGN KEY")),
        "result: {result:?}"
    );
}

#[tokio::test]
async fn an_instrument_has_at_most_one_primary_listing() {
    let pool = db().await;
    let instrument = insert_instrument(&pool, "FI0009000681").await;
    let insert_primary = |symbol: &'static str| {
        sqlx::query_scalar(
            "INSERT INTO instrument_listings (instrument_id, provider, symbol, currency, is_primary)
             VALUES (?, 'yahoo', ?, 'EUR', 1) RETURNING id",
        )
        .bind(instrument)
        .bind(symbol)
        .fetch_one(&pool)
    };
    insert_primary("NOKIA.HE").await.expect("first primary");

    let result: Result<i64, _> = insert_primary("NOKIA.ST").await;

    assert!(is_constraint_violation(&result), "result: {result:?}");
}

#[tokio::test]
async fn decimal_text_round_trips_through_the_database() {
    let pool = db().await;
    let value = Decimal::from_str("1234567.000123").expect("valid decimal");
    sqlx::query("INSERT INTO settings (key, value) VALUES ('amount', ?)")
        .bind(decimal::to_text(value))
        .execute(&pool)
        .await
        .expect("store decimal");

    let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'amount'")
        .fetch_one(&pool)
        .await
        .expect("read decimal");

    assert_eq!(decimal::from_text(&stored), Ok(value));
}

#[tokio::test]
async fn open_creates_the_directory_and_keeps_data_across_reopen() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("nested/saffron.db");
    let first = open(&path).await.expect("first open");
    insert_user(&first, "persisted").await;
    first.close().await;

    let second = open(&path).await.expect("second open");
    let username: String = sqlx::query_scalar("SELECT username FROM users")
        .fetch_one(&second)
        .await
        .expect("read user");

    assert_eq!(username, "persisted");
    second.close().await;
}

#[tokio::test]
async fn open_reports_a_parent_that_is_a_file() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").expect("write the blocker file");

    let result = open(&blocker.join("saffron.db")).await;

    assert!(
        matches!(result, Err(saffron_db::StorageError::CreateDir { .. })),
        "result: {result:?}"
    );
}

#[tokio::test]
async fn open_reports_a_database_path_that_is_a_directory() {
    let dir = tempfile::tempdir().expect("temporary directory");

    let result = open(dir.path()).await;

    assert!(
        matches!(result, Err(saffron_db::StorageError::CreateFile { .. })),
        "result: {result:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn created_directory_and_database_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temporary directory");
    let data_dir = dir.path().join("data");
    let path = data_dir.join("saffron.db");

    let pool = open(&path).await.expect("open");

    let mode =
        |p: &std::path::Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode(&data_dir), 0o700);
    assert_eq!(mode(&path), 0o600);
    pool.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn write_ahead_log_files_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("saffron.db");
    let pool = open(&path).await.expect("open");

    insert_user(&pool, "writer").await;

    let mode = |suffix: &str| {
        let sidecar = dir.path().join(format!("saffron.db{suffix}"));
        std::fs::metadata(&sidecar)
            .expect("sidecar exists while the pool is open")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!((mode("-wal"), mode("-shm")), (0o600, 0o600));
    pool.close().await;
}
