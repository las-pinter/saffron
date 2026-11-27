//! Instruments and the listings that name them at each price provider.

use jiff::Timestamp;
use saffron_core::instrument::InstrumentKind;
use sqlx::SqlitePool;

use crate::time;

/// An instrument with its listings, primary first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    /// Row id.
    pub id: i64,
    /// What it is.
    pub kind: InstrumentKind,
    /// Its name.
    pub name: String,
    /// Its ISIN, when known.
    pub isin: Option<String>,
    /// The ISO 4217 currency it is valued in.
    pub currency: String,
    /// When it was added, as RFC 3339 text.
    pub created_at: String,
    /// Where providers list it.
    pub listings: Vec<Listing>,
}

/// How one provider names an instrument.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Listing {
    /// The provider, such as `yahoo`.
    pub provider: String,
    /// The provider's symbol.
    pub symbol: String,
    /// The exchange as the provider shows it.
    pub exchange: Option<String>,
    /// The currency the provider quotes prices in, such as `GBX` for pence.
    pub currency: String,
    /// Whether prices come from this listing first.
    pub is_primary: bool,
    /// The exchange's IANA time zone, when the provider names it.
    pub timezone: Option<String>,
}

/// The fields of a new instrument.
#[derive(Debug, Clone, Copy)]
pub struct NewInstrument<'a> {
    /// What it is.
    pub kind: InstrumentKind,
    /// Its name.
    pub name: &'a str,
    /// Its ISIN in upper case, when known.
    pub isin: Option<&'a str>,
    /// The ISO 4217 currency it is valued in.
    pub currency: &'a str,
}

/// The fields of a new listing.
#[derive(Debug, Clone, Copy)]
pub struct NewListing<'a> {
    /// The provider.
    pub provider: &'a str,
    /// The provider's symbol.
    pub symbol: &'a str,
    /// The exchange as the provider shows it.
    pub exchange: Option<&'a str>,
    /// The currency the provider quotes prices in.
    pub currency: &'a str,
    /// The exchange's IANA time zone, when the provider names it.
    pub timezone: Option<&'a str>,
}

/// Creates an instrument with `listing` as its primary listing, in one transaction,
/// and returns its id. A listing or ISIN that already exists fails with a
/// unique-constraint error; see [`crate::is_unique_violation`].
pub async fn create(
    pool: &SqlitePool,
    instrument: NewInstrument<'_>,
    listing: NewListing<'_>,
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO instruments (kind, name, isin, currency, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(instrument.kind.as_str())
    .bind(instrument.name)
    .bind(instrument.isin)
    .bind(instrument.currency)
    .bind(time::to_text(now))
    .fetch_one(&mut *tx)
    .await?;
    insert_listing(&mut tx, id, listing).await?;
    tx.commit().await?;
    Ok(id)
}

/// Adds `listing` to instrument `instrument_id`. It becomes the primary listing
/// when the instrument has none.
pub async fn add_listing(
    pool: &SqlitePool,
    instrument_id: i64,
    listing: NewListing<'_>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    insert_listing(&mut tx, instrument_id, listing).await?;
    tx.commit().await
}

async fn insert_listing(
    tx: &mut sqlx::SqliteConnection,
    instrument_id: i64,
    listing: NewListing<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO instrument_listings
             (instrument_id, provider, symbol, exchange, currency, timezone, is_primary)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NOT EXISTS (
             SELECT 1 FROM instrument_listings WHERE instrument_id = ?1 AND is_primary = 1))",
    )
    .bind(instrument_id)
    .bind(listing.provider)
    .bind(listing.symbol)
    .bind(listing.exchange)
    .bind(listing.currency)
    .bind(listing.timezone)
    .execute(tx)
    .await?;
    Ok(())
}

/// Instrument `id` with its listings; `None` when there is none.
pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Instrument>, sqlx::Error> {
    let row: Option<(i64, String, String, Option<String>, String, String)> = sqlx::query_as(
        "SELECT id, kind, name, isin, currency, created_at FROM instruments WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((id, kind, name, isin, currency, created_at)) = row else {
        return Ok(None);
    };
    let kind = InstrumentKind::parse(&kind)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown instrument kind `{kind}`").into()))?;
    let listings = sqlx::query_as(
        "SELECT provider, symbol, exchange, currency, is_primary, timezone FROM instrument_listings
         WHERE instrument_id = ? ORDER BY is_primary DESC, provider, symbol",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(Some(Instrument {
        id,
        kind,
        name,
        isin,
        currency,
        created_at,
        listings,
    }))
}

/// The instrument that `provider` lists as `symbol`.
pub async fn id_by_listing(
    pool: &SqlitePool,
    provider: &str,
    symbol: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT instrument_id FROM instrument_listings WHERE provider = ? AND symbol = ?",
    )
    .bind(provider)
    .bind(symbol)
    .fetch_optional(pool)
    .await
}

/// The instrument with ISIN `isin` (upper case).
pub async fn id_by_isin(pool: &SqlitePool, isin: &str) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM instruments WHERE isin = ?")
        .bind(isin)
        .fetch_optional(pool)
        .await
}

/// Of `symbols` at `provider`, those Saffron already knows, with their instrument.
pub async fn known_symbols(
    pool: &SqlitePool,
    provider: &str,
    symbols: &[&str],
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let symbols =
        serde_json::to_string(symbols).map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    sqlx::query_as(
        "SELECT symbol, instrument_id FROM instrument_listings
         WHERE provider = ?1 AND symbol IN (SELECT value FROM json_each(?2))",
    )
    .bind(provider)
    .bind(symbols)
    .fetch_all(pool)
    .await
}

/// A listing row: instrument id, kind, ISIN, provider symbol, time zone, and
/// quoted currency.
type ListingRow = (
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn price_listing(row: ListingRow) -> Result<PriceListing, sqlx::Error> {
    let (
        instrument_id,
        kind,
        isin,
        symbol,
        timezone,
        currency,
        instrument_currency,
        last_attempt,
        history_attempt,
        history_done,
        timezone_attempt,
    ) = row;
    let kind = InstrumentKind::parse(&kind)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown instrument kind `{kind}`").into()))?;
    Ok(PriceListing {
        instrument_id,
        kind,
        isin,
        symbol,
        timezone,
        currency,
        instrument_currency,
        last_attempt: parse_optional(last_attempt)?,
        history_attempt: parse_optional(history_attempt)?,
        history_done: parse_optional(history_done)?,
        timezone_attempt: parse_optional(timezone_attempt)?,
    })
}

fn parse_optional(text: Option<String>) -> Result<Option<Timestamp>, sqlx::Error> {
    text.map(|text| time::from_text(&text))
        .transpose()
        .map_err(|error| sqlx::Error::Decode(error.to_string().into()))
}

/// An instrument as the price refresh sees it: its listing at one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceListing {
    /// The instrument.
    pub instrument_id: i64,
    /// What it is.
    pub kind: InstrumentKind,
    /// Its ISIN, when known.
    pub isin: Option<String>,
    /// The provider's symbol.
    pub symbol: String,
    /// The exchange's IANA time zone, when known.
    pub timezone: Option<String>,
    /// The currency the provider quotes in, such as `GBX` for pence.
    pub currency: String,
    /// The ISO 4217 currency the instrument is valued in.
    pub instrument_currency: String,
    /// When a quote refresh was last attempted for this listing, price or not.
    pub last_attempt: Option<Timestamp>,
    /// When a history backfill was last attempted for it.
    pub history_attempt: Option<Timestamp>,
    /// When its one-year history was fetched successfully.
    pub history_done: Option<Timestamp>,
    /// When the time zone lookup was last attempted.
    pub timezone_attempt: Option<Timestamp>,
}

/// One listing at `provider` for every instrument that has one, the primary
/// listing first when an instrument has several there.
pub async fn price_listings(
    pool: &SqlitePool,
    provider: &str,
) -> Result<Vec<PriceListing>, sqlx::Error> {
    let rows: Vec<ListingRow> = sqlx::query_as(
        "SELECT i.id, i.kind, i.isin, l.symbol, l.timezone, l.currency, i.currency, l.last_attempt_at, l.history_attempt_at, l.history_done_at,
                l.timezone_attempt_at
         FROM instruments i JOIN instrument_listings l ON l.instrument_id = i.id
         WHERE l.provider = ?
         ORDER BY i.id, l.is_primary DESC, l.symbol",
    )
    .bind(provider)
    .fetch_all(pool)
    .await?;
    let mut listings: Vec<PriceListing> = Vec::with_capacity(rows.len());
    for row in rows {
        let listing = price_listing(row)?;
        if listings
            .last()
            .is_some_and(|last| last.instrument_id == listing.instrument_id)
        {
            continue;
        }
        listings.push(listing);
    }
    Ok(listings)
}

/// Listings at `provider` whose exchange time zone is still unknown.
pub async fn listings_without_timezone(
    pool: &SqlitePool,
    provider: &str,
) -> Result<Vec<PriceListing>, sqlx::Error> {
    let rows: Vec<ListingRow> = sqlx::query_as(
        "SELECT i.id, i.kind, i.isin, l.symbol, l.timezone, l.currency, i.currency, l.last_attempt_at, l.history_attempt_at, l.history_done_at,
                l.timezone_attempt_at
         FROM instruments i JOIN instrument_listings l ON l.instrument_id = i.id
         WHERE l.provider = ? AND l.timezone IS NULL
         ORDER BY i.id",
    )
    .bind(provider)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(price_listing).collect()
}

/// Records the exchange time zone a provider reports for a listing.
pub async fn set_timezone(
    pool: &SqlitePool,
    provider: &str,
    symbol: &str,
    timezone: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE instrument_listings SET timezone = ? WHERE provider = ? AND symbol = ?")
        .bind(timezone)
        .bind(provider)
        .bind(symbol)
        .execute(pool)
        .await?;
    Ok(())
}

/// Records that a refresh was attempted for `symbol` at `provider`, so the next
/// one waits out the retry interval.
pub async fn record_attempt(
    pool: &SqlitePool,
    provider: &str,
    symbol: &str,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE instrument_listings SET last_attempt_at = ? WHERE provider = ? AND symbol = ?",
    )
    .bind(time::to_text(now))
    .bind(provider)
    .bind(symbol)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records that a history backfill was attempted for `symbol` at `provider`.
pub async fn record_history_attempt(
    pool: &SqlitePool,
    provider: &str,
    symbol: &str,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE instrument_listings SET history_attempt_at = ? WHERE provider = ? AND symbol = ?",
    )
    .bind(time::to_text(now))
    .bind(provider)
    .bind(symbol)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records that the one-year history of `symbol` was fetched successfully.
pub async fn record_history_done(
    pool: &SqlitePool,
    provider: &str,
    symbol: &str,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE instrument_listings SET history_done_at = ? WHERE provider = ? AND symbol = ?",
    )
    .bind(time::to_text(now))
    .bind(provider)
    .bind(symbol)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records that the exchange time zone of `symbol` at `provider` was looked up.
pub async fn record_timezone_attempt(
    pool: &SqlitePool,
    provider: &str,
    symbol: &str,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE instrument_listings SET timezone_attempt_at = ? WHERE provider = ? AND symbol = ?",
    )
    .bind(time::to_text(now))
    .bind(provider)
    .bind(symbol)
    .execute(pool)
    .await?;
    Ok(())
}
