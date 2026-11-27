//! Prices: one close per instrument and day. A later quote for the same day
//! replaces the earlier one, except that a price entered by hand is never replaced
//! by a provider.

use jiff::{Timestamp, civil::Date};
use rust_decimal::Decimal;
use sqlx::SqlitePool;

use crate::{decimal, time};

/// The source name of a price entered by hand.
pub const MANUAL: &str = "manual";

/// A stored price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    /// The instrument.
    pub instrument_id: i64,
    /// The trading day.
    pub date: Date,
    /// The close, or the net asset value for a fund.
    pub close: Decimal,
    /// The ISO 4217 currency of [`Self::close`].
    pub currency: String,
    /// Where it came from: a provider name or [`MANUAL`].
    pub source: String,
    /// When the source says the price was valid.
    pub as_of: Timestamp,
    /// When a provider last confirmed it, or when it was entered by hand.
    pub checked_at: Timestamp,
}

/// Stores `price`. Returns `false` when a hand-entered price for that day kept it
/// out.
pub async fn record(pool: &SqlitePool, price: &Price) -> Result<bool, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    insert(&mut connection, price).await
}

/// Stores every price in one transaction; see [`record`]. Returns how many were
/// stored.
pub async fn record_all(pool: &SqlitePool, prices: &[Price]) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut stored = 0;
    for price in prices {
        if insert(&mut tx, price).await? {
            stored += 1;
        }
    }
    tx.commit().await?;
    Ok(stored)
}

async fn insert(
    connection: &mut sqlx::SqliteConnection,
    price: &Price,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO prices (instrument_id, date, close, currency, source, as_of, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (instrument_id, date) DO UPDATE SET
             close = excluded.close, currency = excluded.currency, source = excluded.source,
             as_of = excluded.as_of, checked_at = excluded.checked_at
         WHERE prices.source <> ?8 OR excluded.source = ?8",
    )
    .bind(price.instrument_id)
    .bind(price.date.to_string())
    .bind(decimal::to_text(price.close))
    .bind(&price.currency)
    .bind(&price.source)
    .bind(time::to_text(price.as_of))
    .bind(time::to_text(price.checked_at))
    .bind(MANUAL)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The newest price of `instrument_id`.
pub async fn latest(pool: &SqlitePool, instrument_id: i64) -> Result<Option<Price>, sqlx::Error> {
    let row: Option<PriceRow> = sqlx::query_as(concat!(
        "SELECT instrument_id, date, close, currency, source, as_of, checked_at FROM prices ",
        "WHERE instrument_id = ? ORDER BY date DESC LIMIT 1"
    ))
    .bind(instrument_id)
    .fetch_optional(pool)
    .await?;
    row.map(from_row).transpose()
}

/// The prices of `instrument_id` from `from` on, oldest first.
pub async fn history(
    pool: &SqlitePool,
    instrument_id: i64,
    from: Date,
) -> Result<Vec<Price>, sqlx::Error> {
    let rows: Vec<PriceRow> = sqlx::query_as(concat!(
        "SELECT instrument_id, date, close, currency, source, as_of, checked_at FROM prices ",
        "WHERE instrument_id = ? AND date >= ? ORDER BY date"
    ))
    .bind(instrument_id)
    .bind(from.to_string())
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// The prices of `instrument_ids` up to `to`, oldest first.
pub async fn until(
    pool: &SqlitePool,
    instrument_ids: &[i64],
    to: Date,
) -> Result<Vec<Price>, sqlx::Error> {
    let ids = serde_json::to_string(instrument_ids)
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    let rows: Vec<PriceRow> = sqlx::query_as(concat!(
        "SELECT instrument_id, date, close, currency, source, as_of, checked_at FROM prices ",
        "WHERE instrument_id IN (SELECT value FROM json_each(?1)) AND date <= ?2 ",
        "ORDER BY instrument_id, date"
    ))
    .bind(ids)
    .bind(to.to_string())
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// When each instrument's newest price was valid, and when a provider last
/// confirmed it.
pub async fn latest_marks(
    pool: &SqlitePool,
) -> Result<std::collections::HashMap<i64, (Timestamp, Timestamp)>, sqlx::Error> {
    let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT p.instrument_id, p.as_of, p.checked_at FROM prices p
         WHERE p.date = (SELECT max(q.date) FROM prices q WHERE q.instrument_id = p.instrument_id)",
    )
    .fetch_all(pool)
    .await?;
    let mut marks = std::collections::HashMap::with_capacity(rows.len());
    for (instrument_id, as_of, checked_at) in rows {
        let decode = |error| sqlx::Error::Decode(error);
        let as_of = time::from_text(&as_of).map_err(|e| decode(e.to_string().into()))?;
        let checked_at = checked_at
            .map(|text| time::from_text(&text))
            .transpose()
            .map_err(|e| decode(e.to_string().into()))?
            .unwrap_or(as_of);
        marks.insert(instrument_id, (as_of, checked_at));
    }
    Ok(marks)
}

type PriceRow = (i64, String, String, String, String, String, Option<String>);

fn from_row(row: PriceRow) -> Result<Price, sqlx::Error> {
    let (instrument_id, date, close, currency, source, as_of, checked_at) = row;
    let decode = |error: String| sqlx::Error::Decode(error.into());
    let as_of = time::from_text(&as_of).map_err(|error| decode(error.to_string()))?;
    // Rows from before checked_at existed count as confirmed when they were valid.
    let checked_at = checked_at
        .map(|text| time::from_text(&text).map_err(|error| decode(error.to_string())))
        .transpose()?
        .unwrap_or(as_of);
    Ok(Price {
        instrument_id,
        date: date
            .parse()
            .map_err(|error| decode(format!("bad price date `{date}`: {error}")))?,
        close: decimal::from_text(&close).map_err(|error| decode(error.to_string()))?,
        currency,
        source,
        as_of,
        checked_at,
    })
}
