//! The scheduled price refresh: batched Yahoo quotes during exchange hours, the
//! FT fund fallback when Yahoo has no price, and a one-year history for new
//! instruments.

use jiff::{Timestamp, civil::Date, tz::TimeZone};
use saffron_core::{
    freshness::{QUOTE_INTERVAL, fund_due, in_window},
    instrument::InstrumentKind,
};
use saffron_db::{
    instruments::{self, PriceListing},
    prices::{self, Price},
};
use saffron_market::{History, MarketError, Quote, Spot, ft, normalize, yahoo};

use crate::{error::ApiResult, settings::ft_fallback, state::AppState};

/// Most symbols one quote request carries.
pub const QUOTE_BATCH: usize = 20;
/// How far back a new instrument's history goes.
pub const HISTORY_DAYS: i64 = 365;
/// How long a failed time zone lookup waits before another try.
pub const TIMEZONE_RETRY: jiff::SignedDuration = jiff::SignedDuration::from_hours(24);
/// How long a failed history fetch waits before another try.
pub const HISTORY_RETRY: jiff::SignedDuration = jiff::SignedDuration::from_hours(1);
/// How often the refresh runs.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// What one tick stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Quotes stored from a provider.
    pub quotes: usize,
    /// Prices stored by history backfill.
    pub history: usize,
    /// Exchange rates stored.
    pub fx: usize,
    /// Listings whose refresh failed.
    pub failed: usize,
}

/// Runs one refresh: history for new instruments, time zones for old listings,
/// then the due quotes and the fund fallback.
///
/// History comes first, and backfill tracks its own completion per listing, so a
/// quote stored before the history never strands the instrument without one.
pub async fn tick(state: &AppState) -> ApiResult<Refreshed> {
    let now = state.clock.now();
    let history = backfill(state, now).await?;
    let fx = crate::fx_refresh::refresh(state, now).await?;
    fill_timezones(state).await?;
    let due = due_listings(state, now).await?;
    let quotes = refresh_quotes(state, &due, now).await?;
    Ok(Refreshed {
        quotes,
        history,
        fx,
        failed: due.len().saturating_sub(quotes),
    })
}

/// Starts the refresh loop until the process ends.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match tick(&state).await {
                Ok(done) if done != Refreshed::default() => {
                    tracing::info!(?done, "prices refreshed");
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "price refresh failed"),
            }
        }
    });
}

/// The Yahoo listings that are due: those whose exchange window is open and whose
/// newest price is stale enough to refresh, and funds after their daily slot.
///
/// A listing is not asked for again within [`QUOTE_INTERVAL`] of its last attempt,
/// whether or not that attempt produced a price, so a symbol the provider has
/// nothing for does not multiply requests.
async fn due_listings(state: &AppState, now: Timestamp) -> ApiResult<Vec<PriceListing>> {
    let listings = instruments::price_listings(&state.pool, yahoo::PROVIDER).await?;
    let marks = prices::latest_marks(&state.pool).await?;
    let mut due = Vec::new();
    for listing in listings {
        if listing
            .last_attempt
            .is_some_and(|at| now.duration_since(at) < QUOTE_INTERVAL)
        {
            continue;
        }
        let kind = listing.kind;
        let timezone = timezone_of(state, &listing);
        let marks = marks.get(&listing.instrument_id).copied();
        let is_due = if kind == InstrumentKind::Fund {
            // An attempt counts as confirmation, so a fund whose price is missing is
            // still asked for once per evening slot, not every interval.
            fund_due(now, listing.last_attempt, &state.timezone)
        } else {
            // A quote is due when the market time of the newest price is older
            // than the refresh interval; fetching a history must not hold that off.
            in_window(kind, now, &timezone)
                && marks.is_none_or(|(as_of, _)| now.duration_since(as_of) >= QUOTE_INTERVAL)
        };
        if is_due {
            due.push(listing);
        }
    }
    Ok(due)
}

/// Learns the exchange time zone of listings stored before time zones were saved.
async fn fill_timezones(state: &AppState) -> ApiResult<()> {
    let listings = instruments::listings_without_timezone(&state.pool, yahoo::PROVIDER).await?;
    let now = state.clock.now();
    for listing in listings {
        if listing
            .timezone_attempt
            .is_some_and(|at| now.duration_since(at) < TIMEZONE_RETRY)
        {
            continue;
        }
        instruments::record_timezone_attempt(&state.pool, yahoo::PROVIDER, &listing.symbol, now)
            .await?;
        match state.market.yahoo.details(&listing.symbol).await {
            Ok(details) => {
                if let Some(timezone) = details.timezone {
                    instruments::set_timezone(
                        &state.pool,
                        yahoo::PROVIDER,
                        &listing.symbol,
                        &timezone,
                    )
                    .await?;
                }
            }
            Err(MarketError::NotFound) => {}
            Err(error) => tracing::warn!(%error, symbol = %listing.symbol, "no time zone"),
        }
    }
    Ok(())
}

/// Quotes `due` in batches, then asks FT for funds that are still missing.
async fn refresh_quotes(
    state: &AppState,
    due: &[PriceListing],
    now: Timestamp,
) -> ApiResult<usize> {
    let mut stored = 0;
    let mut failed: Vec<&PriceListing> = Vec::new();
    for batch in due.chunks(QUOTE_BATCH) {
        let symbols: Vec<&str> = batch
            .iter()
            .map(|listing| listing.symbol.as_str())
            .collect();
        match state.market.yahoo.quotes(&symbols).await {
            Ok(spots) => {
                stored += store_spots(state, batch, &spots, now).await?;
                for listing in batch {
                    let spot = spots.iter().find(|spot| spot.symbol == listing.symbol);
                    let usable =
                        spot.is_some_and(|spot| !stale_fund_spot(state, listing, spot, now));
                    if !usable {
                        failed.push(listing);
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Yahoo quotes failed");
                failed.extend(batch);
            }
        }
        for listing in batch {
            instruments::record_attempt(&state.pool, yahoo::PROVIDER, &listing.symbol, now).await?;
        }
    }
    stored += fund_fallback(state, &failed, now).await?;
    Ok(stored)
}

/// Whether a fund's Yahoo price is too old to count, so FT should be asked instead.
fn stale_fund_spot(state: &AppState, listing: &PriceListing, spot: &Spot, now: Timestamp) -> bool {
    if listing.kind != InstrumentKind::Fund {
        return false;
    }
    let timezone = timezone_of(state, listing);
    let today = now.to_zoned_datetime(timezone.clone()).date();
    saffron_core::freshness::business_days_after(spot.at.to_zoned_datetime(timezone).date(), today)
        > saffron_core::freshness::STALE_AFTER_BUSINESS_DAYS
}

async fn store_spots(
    state: &AppState,
    batch: &[PriceListing],
    spots: &[Spot],
    now: Timestamp,
) -> ApiResult<usize> {
    let mut prices = Vec::new();
    for spot in spots {
        let Some(listing) = batch.iter().find(|listing| listing.symbol == spot.symbol) else {
            continue;
        };
        let Some(price) = price_from_spot(state, listing, spot, now) else {
            tracing::warn!(
                symbol = %listing.symbol,
                currency = %listing.currency,
                "a quote's currency is unreadable; skipped"
            );
            continue;
        };
        prices.push(price);
    }
    Ok(prices::record_all(&state.pool, &prices).await?)
}

fn price_from_spot(
    state: &AppState,
    listing: &PriceListing,
    spot: &Spot,
    now: Timestamp,
) -> Option<Price> {
    let timezone = timezone_of(state, listing);
    let date = spot.at.to_zoned_datetime(timezone).date();
    let (currency, close) = normalize(&listing.currency, spot.close)?;
    Some(Price {
        instrument_id: listing.instrument_id,
        date,
        close,
        currency,
        source: yahoo::PROVIDER.to_owned(),
        as_of: spot.at,
        checked_at: now,
    })
}

/// Asks FT for the funds among `missing`, when an administrator has turned the
/// fallback on and the fund has an ISIN.
async fn fund_fallback(
    state: &AppState,
    missing: &[&PriceListing],
    now: Timestamp,
) -> ApiResult<usize> {
    let funds: Vec<&&PriceListing> = missing
        .iter()
        .filter(|listing| listing.kind == InstrumentKind::Fund && listing.isin.is_some())
        .collect();
    if funds.is_empty() || !ft_fallback(state).await? {
        return Ok(0);
    }
    let today = now.to_zoned_datetime(state.timezone.clone()).date();
    let mut prices = Vec::new();
    for listing in funds {
        let Some(isin) = listing
            .isin
            .as_deref()
            .and_then(saffron_core::instrument::Isin::parse)
        else {
            continue;
        };
        match state.market.ft.find_fund(&isin).await {
            Ok(fund) if fund.currency != listing.instrument_currency => {
                tracing::warn!(
                    isin = %isin,
                    fund_currency = %fund.currency,
                    instrument_currency = %listing.instrument_currency,
                    "FT lists this fund in another currency; not stored"
                );
            }
            Ok(fund) => match state.market.ft.latest(&fund, today).await {
                Ok(quote) => {
                    record_ft_listing(state, listing, &fund.symbol, &fund.currency).await?;
                    prices.push(price_from_quote(listing, &quote, now));
                }
                Err(MarketError::NotFound) => {}
                Err(error) => tracing::warn!(%error, isin = %isin, "FT has no NAV"),
            },
            Err(MarketError::NotFound) => {}
            Err(error) => tracing::warn!(%error, isin = %isin, "FT search failed"),
        }
    }
    Ok(prices::record_all(&state.pool, &prices).await?)
}

/// Remembers where FT lists `instrument`, for visibility; the search runs again on
/// each fallback since the stored listing holds no FT id.
async fn record_ft_listing(
    state: &AppState,
    listing: &PriceListing,
    symbol: &str,
    currency: &str,
) -> ApiResult<()> {
    if instruments::id_by_listing(&state.pool, ft::PROVIDER, symbol)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let new = instruments::NewListing {
        provider: ft::PROVIDER,
        symbol,
        exchange: None,
        currency,
        timezone: None,
    };
    if let Err(error) = instruments::add_listing(&state.pool, listing.instrument_id, new).await
        && !saffron_db::is_unique_violation(&error)
    {
        return Err(error.into());
    }
    Ok(())
}

fn price_from_quote(listing: &PriceListing, quote: &Quote, now: Timestamp) -> Price {
    Price {
        instrument_id: listing.instrument_id,
        date: quote.date,
        close: quote.close,
        currency: quote.currency.clone(),
        source: ft::PROVIDER.to_owned(),
        as_of: quote.as_of,
        checked_at: now,
    }
}

/// Fetches a year of history for every instrument that has no price at all.
async fn backfill(state: &AppState, now: Timestamp) -> ApiResult<usize> {
    let listings = instruments::price_listings(&state.pool, yahoo::PROVIDER).await?;
    let unhistoried: Vec<&PriceListing> = listings
        .iter()
        .filter(|listing| listing.history_done.is_none())
        .collect();
    if unhistoried.is_empty() {
        return Ok(0);
    }
    let today = now.to_zoned_datetime(state.timezone.clone()).date();
    let Ok(from) = today.checked_sub(jiff::Span::new().days(HISTORY_DAYS)) else {
        return Ok(0);
    };
    let mut stored = 0;
    for listing in unhistoried {
        if listing
            .history_attempt
            .is_some_and(|at| now.duration_since(at) < HISTORY_RETRY)
        {
            continue;
        }
        instruments::record_history_attempt(&state.pool, yahoo::PROVIDER, &listing.symbol, now)
            .await?;
        match state
            .market
            .yahoo
            .history(&listing.symbol, from, today)
            .await
        {
            Ok(history) => {
                stored += store_history(state, listing.instrument_id, &history, now).await?;
                instruments::record_history_done(
                    &state.pool,
                    yahoo::PROVIDER,
                    &listing.symbol,
                    now,
                )
                .await?;
            }
            Err(MarketError::NotFound) => {
                // The provider has no history for it; asking again will not help.
                instruments::record_history_done(
                    &state.pool,
                    yahoo::PROVIDER,
                    &listing.symbol,
                    now,
                )
                .await?;
            }
            Err(error) => tracing::warn!(%error, symbol = %listing.symbol, "Yahoo history failed"),
        }
    }
    Ok(stored)
}

async fn store_history(
    state: &AppState,
    instrument_id: i64,
    history: &History,
    now: Timestamp,
) -> ApiResult<usize> {
    let prices: Vec<Price> = history
        .closes
        .iter()
        .map(|close| Price {
            instrument_id,
            date: close.date,
            close: close.close,
            currency: history.currency.clone(),
            source: yahoo::PROVIDER.to_owned(),
            as_of: start_of(close.date),
            checked_at: now,
        })
        .collect();
    Ok(prices::record_all(&state.pool, &prices).await?)
}

fn timezone_of(state: &AppState, listing: &PriceListing) -> TimeZone {
    listing
        .timezone
        .as_deref()
        .and_then(|name| TimeZone::get(name).ok())
        .unwrap_or_else(|| state.timezone.clone())
}

fn start_of(date: Date) -> Timestamp {
    TimeZone::UTC
        .to_timestamp(date.to_datetime(jiff::civil::Time::midnight()))
        .unwrap_or(Timestamp::UNIX_EPOCH)
}
