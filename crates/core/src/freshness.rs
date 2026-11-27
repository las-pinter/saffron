//! When prices are refreshed and when a price counts as stale.
//!
//! Exchange hours are not stored. Instead every exchange gets one generous
//! window, [`WINDOW_START`] to [`WINDOW_END`] on weekdays in its own time zone,
//! which covers the regular session of the exchanges Saffron targets with room for
//! the provider's delay. Holidays are not known, so a holiday counts as a trading
//! day.

use jiff::{
    SignedDuration, Timestamp,
    civil::{Date, Time, Weekday},
    tz::TimeZone,
};

use crate::instrument::InstrumentKind;

/// When the daily refresh window opens, exchange-local.
pub const WINDOW_START: Time = Time::constant(7, 0, 0, 0);
/// When the daily refresh window closes, exchange-local.
pub const WINDOW_END: Time = Time::constant(22, 0, 0, 0);
/// How often quotes are refreshed inside the window.
pub const QUOTE_INTERVAL: SignedDuration = SignedDuration::from_mins(15);
/// Inside the window, a quote whose own time is older than this is stale.
pub const QUOTE_STALE_AFTER: SignedDuration = SignedDuration::from_mins(15);
/// When the daily fund refresh falls due, installation-local.
pub const FUND_SLOT_HOUR: i8 = 20;
/// A price whose day is more than this many business days old is stale.
pub const STALE_AFTER_BUSINESS_DAYS: i64 = 3;

/// Whether `now` falls in the refresh window of an exchange in `tz`. Crypto trades
/// around the clock, so its window never closes.
#[must_use]
pub fn in_window(kind: InstrumentKind, now: Timestamp, tz: &TimeZone) -> bool {
    if kind == InstrumentKind::Crypto {
        return true;
    }
    let local = now.to_zoned_datetime(tz.clone());
    is_business_day(local.date()) && (WINDOW_START..WINDOW_END).contains(&local.time())
}

/// Whether a price is stale at `now`.
///
/// - Its day is more than [`STALE_AFTER_BUSINESS_DAYS`] business days before
///   today in `tz`.
/// - When `intraday` is set: inside the window, the price's own time is more than
///   [`QUOTE_STALE_AFTER`] before `now`. Fund NAVs and hand-entered prices are
///   judged by the day alone, since neither carries a market time.
#[must_use]
pub fn is_stale(
    kind: InstrumentKind,
    price_date: Date,
    as_of: Timestamp,
    now: Timestamp,
    tz: &TimeZone,
    intraday: bool,
) -> bool {
    let today = now.to_zoned_datetime(tz.clone()).date();
    if business_days_after(price_date, today) > STALE_AFTER_BUSINESS_DAYS {
        return true;
    }
    intraday && in_window(kind, now, tz) && now.duration_since(as_of) > QUOTE_STALE_AFTER
}

/// Whether a fund's daily NAV falls due at `now`: after [`FUND_SLOT_HOUR`] in
/// `tz`, and not yet fetched since that hour today.
#[must_use]
pub fn fund_due(now: Timestamp, checked_at: Option<Timestamp>, tz: &TimeZone) -> bool {
    let local = now.to_zoned_datetime(tz.clone());
    if local.hour() < FUND_SLOT_HOUR || !is_business_day(local.date()) {
        return false;
    }
    match checked_at {
        None => true,
        Some(checked) => {
            checked.to_zoned_datetime(tz.clone()).date() < local.date()
                || (checked.to_zoned_datetime(tz.clone()).hour() < FUND_SLOT_HOUR
                    && checked.to_zoned_datetime(tz.clone()).date() == local.date())
        }
    }
}

/// The business days (Monday to Friday) after `from`, up to and including `to`.
/// Zero when `to` is not after `from`.
#[must_use]
pub fn business_days_after(from: Date, to: Date) -> i64 {
    let mut count = 0;
    let mut day = from;
    while day < to {
        day = day.tomorrow().unwrap_or(to);
        if is_business_day(day) {
            count += 1;
        }
    }
    count
}

/// Whether `day` is a weekday (Monday to Friday).
#[must_use]
pub fn is_business_day(day: Date) -> bool {
    !matches!(day.weekday(), Weekday::Saturday | Weekday::Sunday)
}
