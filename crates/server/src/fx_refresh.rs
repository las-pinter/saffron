//! The daily ECB reference-rate refresh.
//!
//! The ECB publishes once each working day, about 16:00 in Brussels. The first
//! run takes the whole published history so old transaction dates convert with
//! their own day's rate; a server that missed days repairs the gap the same way.

use jiff::{SignedDuration, Timestamp, tz::TimeZone};
use saffron_core::freshness::{business_days_after, is_business_day};
use saffron_db::{
    fx::{self, FxRate},
    settings,
};

use crate::{error::ApiResult, state::AppState};

/// When the ECB publishes, in its own time zone.
pub const FX_PUBLICATION_HOUR: i8 = 16;
/// The day whose rates are stored, once a fetch went through.
pub const FX_ATTEMPTED_ON: &str = "fx.attempted_on";
/// When the ECB was last asked, so a stale or failed fetch retries later.
pub const FX_ATTEMPTED_AT: &str = "fx.attempted_at";
/// How long a failed or stale fetch waits before another try.
pub const FX_RETRY: SignedDuration = SignedDuration::from_hours(1);
/// The ECB's own time zone.
pub const FX_TIMEZONE: &str = "Europe/Brussels";

/// Fetches the rates if they are due. Returns how many rows were stored.
pub async fn refresh(state: &AppState, now: Timestamp) -> ApiResult<usize> {
    let Some(brussels) = TimeZone::get(FX_TIMEZONE).ok() else {
        return Ok(0);
    };
    let local = now.to_zoned_datetime(brussels);
    let today = local.date();
    let latest = fx::latest_date(&state.pool).await?;
    // A fresh install takes the archive at once: it is published already, so the
    // publication hour only governs the daily file.
    if latest.is_some() && local.hour() < FX_PUBLICATION_HOUR {
        return Ok(0);
    }
    if settings::get(&state.pool, FX_ATTEMPTED_ON)
        .await?
        .as_deref()
        == Some(&today.to_string())
    {
        return Ok(0);
    }
    if recent_attempt(state, now).await? {
        return Ok(0);
    }
    settings::set(
        &state.pool,
        FX_ATTEMPTED_AT,
        &saffron_db::time::to_text(now),
    )
    .await?;

    if latest == Some(today) {
        settings::set(&state.pool, FX_ATTEMPTED_ON, &today.to_string()).await?;
        return Ok(0);
    }
    // A gap of more than one working day, or no rates at all, needs the archive.
    let needs_history = latest.is_none_or(|latest| business_days_after(latest, today) > 1);
    let (stored, stored_day) = if needs_history {
        match state.market.ecb.history().await {
            Ok(days) => {
                let newest = days.iter().map(|day| day.date).max();
                let rows: Vec<FxRate> = days.into_iter().flat_map(all_rates).collect();
                (fx::upsert(&state.pool, &rows).await?, newest)
            }
            Err(error) => {
                tracing::warn!(%error, "ECB history failed");
                (0, None)
            }
        }
    } else {
        match state.market.ecb.daily().await {
            Ok(day) => {
                let date = day.date;
                (fx::upsert(&state.pool, &all_rates(day)).await?, Some(date))
            }
            Err(error) => {
                tracing::warn!(%error, "ECB daily rates failed");
                (0, None)
            }
        }
    };
    // The day counts as done only when a fetch succeeds and reaches today, or when
    // the ECB publishes nothing today. A lagging, failed, or weekend-broken fetch
    // stays due and retries after the interval.
    let reached_today = stored_day.is_some_and(|date| date >= today || !is_business_day(today));
    if reached_today {
        settings::set(&state.pool, FX_ATTEMPTED_ON, &today.to_string()).await?;
    }
    Ok(stored)
}

async fn recent_attempt(state: &AppState, now: Timestamp) -> ApiResult<bool> {
    let Some(text) = settings::get(&state.pool, FX_ATTEMPTED_AT).await? else {
        return Ok(false);
    };
    let Ok(at) = text.parse::<Timestamp>() else {
        return Ok(false);
    };
    Ok(now.duration_since(at) < FX_RETRY)
}

/// One published day as stored rows.
fn all_rates(day: saffron_market::ecb::Rates) -> Vec<FxRate> {
    day.per_eur
        .into_iter()
        .map(|(currency, per_eur)| FxRate {
            date: day.date,
            currency,
            per_eur,
        })
        .collect()
}
