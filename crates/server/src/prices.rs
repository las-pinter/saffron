//! Prices over HTTP: the latest price with its stale flag, the history, and
//! prices entered by hand when no provider has one.

use std::str::FromStr;

use axum::{Json, extract::State};
use jiff::{Timestamp, civil::Date, tz::TimeZone};
use rust_decimal::Decimal;
use saffron_core::freshness;
use saffron_db::{
    audit,
    instruments::{self, Instrument},
    prices::{self, MANUAL, Price},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    request::{JsonBody, PathParam, QueryParam},
    state::AppState,
    users::record_audit,
};

/// How far back the history goes when the request names no start.
pub const DEFAULT_HISTORY_DAYS: i64 = 365;

/// The latest price of an instrument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceView {
    /// The trading day.
    pub date: String,
    /// The close or NAV, as decimal text.
    pub close: String,
    /// The ISO 4217 currency of `close`.
    pub currency: String,
    /// A provider name, or `manual`.
    pub source: String,
    /// When the source says the price was valid.
    pub as_of: String,
    /// When a provider last confirmed it.
    pub checked_at: String,
    /// Whether it is older than the freshness rules allow.
    pub stale: bool,
}

/// One day of an instrument's price history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPoint {
    /// The trading day.
    pub date: String,
    /// The close or NAV, as decimal text.
    pub close: String,
    /// The ISO 4217 currency of `close`.
    pub currency: String,
    /// A provider name, or `manual`.
    pub source: String,
}

/// Query of `GET /api/instruments/{id}/prices`.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    from: Option<String>,
}

/// Body of `PUT /api/instruments/{id}/prices/{date}`.
#[derive(Debug, Deserialize)]
pub struct ManualPrice {
    close: String,
}

/// The time zone an instrument trades in: its primary listing's, else the
/// installation's.
#[must_use]
pub fn trading_timezone(state: &AppState, instrument: &Instrument) -> TimeZone {
    instrument
        .listings
        .first()
        .and_then(|listing| listing.timezone.as_deref())
        .and_then(|name| TimeZone::get(name).ok())
        .unwrap_or_else(|| state.timezone.clone())
}

/// The latest price of `instrument` with its stale flag at the current time.
pub async fn latest_view(
    state: &AppState,
    instrument: &Instrument,
) -> ApiResult<Option<PriceView>> {
    let Some(price) = prices::latest(&state.pool, instrument.id).await? else {
        return Ok(None);
    };
    let out_of_date = freshness::is_stale(
        instrument.kind,
        price.date,
        price.as_of,
        state.clock.now(),
        &trading_timezone(state, instrument),
        intraday_rule(instrument, &price),
    );
    Ok(Some(PriceView {
        date: price.date.to_string(),
        close: saffron_db::decimal::to_text(price.close),
        currency: price.currency,
        source: price.source,
        as_of: saffron_db::time::to_text(price.as_of),
        checked_at: saffron_db::time::to_text(price.checked_at),
        stale: out_of_date,
    }))
}

/// `GET /api/instruments/{id}/prices?from=YYYY-MM-DD`: oldest first; the last
/// [`DEFAULT_HISTORY_DAYS`] without `from`.
pub async fn history(
    State(state): State<AppState>,
    _: CurrentUser,
    PathParam(id): PathParam<i64>,
    QueryParam(query): QueryParam<HistoryQuery>,
) -> ApiResult<Json<Vec<HistoryPoint>>> {
    let instrument = existing(&state, id).await?;
    let from = match query.from.as_deref() {
        Some(text) => parse_date(text)?,
        None => today(&state)
            .checked_sub(jiff::Span::new().days(DEFAULT_HISTORY_DAYS))
            .map_err(ApiError::internal)?,
    };
    let points = prices::history(&state.pool, instrument.id, from).await?;
    Ok(Json(
        points
            .into_iter()
            .map(|price| HistoryPoint {
                date: price.date.to_string(),
                close: saffron_db::decimal::to_text(price.close),
                currency: price.currency,
                source: price.source,
            })
            .collect(),
    ))
}

/// `PUT /api/instruments/{id}/prices/{date}`: enters the price of a day by hand, in
/// the instrument's currency. Providers never overwrite it.
pub async fn enter(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    PathParam((id, date)): PathParam<(i64, String)>,
    JsonBody(body): JsonBody<ManualPrice>,
) -> ApiResult<Json<PriceView>> {
    let instrument = existing(&state, id).await?;
    let date = parse_date(&date)?;
    if date > today(&state) {
        return Err(ApiError::bad_request(
            "future_date",
            "a price cannot be entered for a day that has not come",
        ));
    }
    let close = parse_close(&body.close)?;
    let price = Price {
        instrument_id: instrument.id,
        date,
        close,
        currency: instrument.currency.clone(),
        source: MANUAL.to_owned(),
        as_of: start_of(date),
        checked_at: state.clock.now(),
    };
    prices::record(&state.pool, &price).await?;
    let detail = serde_json::json!({
        "date": date.to_string(),
        "close": saffron_db::decimal::to_text(close),
    })
    .to_string();
    let entry = audit::Entry {
        user_id: Some(session.user.id),
        action: "price.manual",
        target: Some(&instrument.name),
        detail: Some(&detail),
    };
    record_audit(&state, entry).await;
    for written in prices::history(&state.pool, instrument.id, date).await? {
        if written.date == date {
            return Ok(Json(day_view(
                &instrument,
                &written,
                state.clock.now(),
                &trading_timezone(&state, &instrument),
            )));
        }
    }
    Err(ApiError::internal("an entered price cannot be read back"))
}

/// Whether the intraday rule applies: fund NAVs and hand-entered prices carry no
/// market time, so only the day counts for them.
fn intraday_rule(instrument: &Instrument, price: &Price) -> bool {
    instrument.kind != saffron_core::instrument::InstrumentKind::Fund && price.source != MANUAL
}

/// `price` as the API shows it, judged stale at `now` in the installation zone.
fn day_view(instrument: &Instrument, price: &Price, now: Timestamp, tz: &TimeZone) -> PriceView {
    let out_of_date = freshness::is_stale(
        instrument.kind,
        price.date,
        price.as_of,
        now,
        tz,
        intraday_rule(instrument, price),
    );
    PriceView {
        date: price.date.to_string(),
        close: saffron_db::decimal::to_text(price.close),
        currency: price.currency.clone(),
        source: price.source.clone(),
        as_of: saffron_db::time::to_text(price.as_of),
        checked_at: saffron_db::time::to_text(price.checked_at),
        stale: out_of_date,
    }
}

async fn existing(state: &AppState, id: i64) -> ApiResult<Instrument> {
    instruments::get(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)
}

fn today(state: &AppState) -> Date {
    state
        .clock
        .now()
        .to_zoned_datetime(state.timezone.clone())
        .date()
}

fn start_of(date: Date) -> Timestamp {
    TimeZone::UTC
        .to_timestamp(date.to_datetime(jiff::civil::Time::midnight()))
        .unwrap_or(Timestamp::UNIX_EPOCH)
}

fn parse_date(text: &str) -> ApiResult<Date> {
    text.parse()
        .map_err(|_| ApiError::bad_request("invalid_date", "a date is written YYYY-MM-DD"))
}

/// A positive decimal written with digits and at most one `.`.
fn parse_close(text: &str) -> ApiResult<Decimal> {
    let invalid = || {
        ApiError::bad_request(
            "invalid_price",
            "a price is a positive decimal number, such as 136.87",
        )
    };
    let text = text.trim();
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(invalid());
    }
    let close = Decimal::from_str(text).map_err(|_| invalid())?;
    if close <= Decimal::ZERO {
        return Err(invalid());
    }
    Ok(close)
}
