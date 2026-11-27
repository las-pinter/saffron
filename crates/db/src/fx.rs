//! Foreign exchange: the ECB's daily reference rates, stored as units of a
//! currency per one euro, and conversion between currencies through the euro.

use std::collections::BTreeMap;

use jiff::civil::Date;
use rust_decimal::Decimal;
use sqlx::SqlitePool;

use crate::decimal;

/// The source name Saffron stores for ECB rates.
pub const ECB: &str = "ecb";

/// One currency's rate on one day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxRate {
    /// The publication day.
    pub date: Date,
    /// The ISO 4217 code.
    pub currency: String,
    /// Units of `currency` per one euro.
    pub per_eur: Decimal,
}

/// The rates of one day, keyed by currency.
pub type Day = BTreeMap<String, Decimal>;

/// Stores `rates`, replacing any earlier value for the same day.
///
/// Each element is a day and its rates.
pub async fn upsert(pool: &SqlitePool, rates: &[FxRate]) -> Result<usize, sqlx::Error> {
    // The full history is hundreds of thousands of rows; committing in chunks keeps
    // one transaction from holding the write lock for the whole archive.
    let mut stored = 0;
    for chunk in rates.chunks(UPSERT_CHUNK) {
        stored += upsert_chunk(pool, chunk).await?;
    }
    Ok(stored)
}

async fn upsert_chunk(pool: &SqlitePool, rates: &[FxRate]) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut stored = 0;
    for rate in rates {
        let result = sqlx::query(
            "INSERT INTO fx_rates (date, currency, per_eur, source) VALUES (?, ?, ?, ?)
             ON CONFLICT (date, currency) DO UPDATE SET
                 per_eur = excluded.per_eur, source = excluded.source",
        )
        .bind(rate.date.to_string())
        .bind(&rate.currency)
        .bind(decimal::to_text(rate.per_eur))
        .bind(ECB)
        .execute(&mut *tx)
        .await?;
        stored += usize::try_from(result.rows_affected()).unwrap_or(usize::MAX);
    }
    tx.commit().await?;
    Ok(stored)
}

/// Rows written per transaction.
pub const UPSERT_CHUNK: usize = 2000;

/// The newest day with any stored rate.
pub async fn latest_date(pool: &SqlitePool) -> Result<Option<Date>, sqlx::Error> {
    let text: Option<String> = sqlx::query_scalar("SELECT max(date) FROM fx_rates")
        .fetch_one(pool)
        .await?;
    match text {
        None => Ok(None),
        Some(text) => text
            .parse()
            .map(Some)
            .map_err(|error| sqlx::Error::Decode(format!("bad fx date `{text}`: {error}").into())),
    }
}

/// The rates of `date`, or of the newest day before it when that day has none.
/// Weekends and holidays carry the last published rates.
pub async fn day_on_or_before(pool: &SqlitePool, date: Date) -> Result<Option<Day>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT currency, per_eur FROM fx_rates
         WHERE date = (SELECT max(date) FROM fx_rates WHERE date <= ?)",
    )
    .bind(date.to_string())
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut day = Day::new();
    for (currency, per_eur) in rows {
        let per_eur = decimal::from_text(&per_eur)
            .map_err(|error| sqlx::Error::Decode(error.to_string().into()))?;
        day.insert(currency, per_eur);
    }
    Ok(Some(day))
}

/// The ECB days from the newest one on or before `from` through `to`, oldest
/// first; the leading day lets a value ramp start with a carried rate.
pub async fn between_with_prior(
    pool: &SqlitePool,
    from: Date,
    to: Date,
) -> Result<Vec<(Date, Day)>, sqlx::Error> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT date, currency, per_eur FROM fx_rates
         WHERE date >= COALESCE((SELECT max(date) FROM fx_rates WHERE date <= ?1), ?1)
           AND date <= ?2
         ORDER BY date, currency",
    )
    .bind(from.to_string())
    .bind(to.to_string())
    .fetch_all(pool)
    .await?;
    let mut days: Vec<(Date, Day)> = Vec::new();
    for (date, currency, per_eur) in rows {
        let day = date.parse().map_err(|error| {
            sqlx::Error::Decode(format!("bad fx date `{date}`: {error}").into())
        })?;
        let per_eur = decimal::from_text(&per_eur)
            .map_err(|error| sqlx::Error::Decode(error.to_string().into()))?;
        match days.last_mut() {
            Some((last, rates)) if *last == day => {
                rates.insert(currency, per_eur);
            }
            _ => {
                let mut rates = Day::new();
                rates.insert(currency, per_eur);
                days.push((day, rates));
            }
        }
    }
    Ok(days)
}

/// `amount` in `from` converted to `to` on `date` (or the last published day
/// before it). `None` when a rate is missing, so a wrong number is never invented.
pub async fn convert(
    pool: &SqlitePool,
    amount: Decimal,
    from: &str,
    to: &str,
    date: Date,
) -> Result<Option<Decimal>, sqlx::Error> {
    if from == to {
        return Ok(Some(amount));
    }
    let Some(day) = day_on_or_before(pool, date).await? else {
        return Ok(None);
    };
    Ok(convert_with(&day, amount, from, to))
}

/// The pure conversion, for a day already in hand.
#[must_use]
pub fn convert_with(day: &Day, amount: Decimal, from: &str, to: &str) -> Option<Decimal> {
    if from == to {
        return Some(amount);
    }
    let in_euro = match from {
        "EUR" => amount,
        code => amount.checked_div(*day.get(code)?)?,
    };
    match to {
        "EUR" => Some(in_euro),
        code => in_euro.checked_mul(*day.get(code)?),
    }
}
