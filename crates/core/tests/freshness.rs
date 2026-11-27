//! The refresh window and the stale rules, in real exchange time zones.

use jiff::{
    Timestamp,
    civil::{Date, date},
    tz::TimeZone,
};
use saffron_core::{
    freshness::{business_days_after, fund_due, in_window, is_stale},
    instrument::InstrumentKind::{Crypto, Fund, Stock},
};

fn at(text: &str) -> Timestamp {
    text.parse().expect("timestamp")
}

fn helsinki() -> TimeZone {
    TimeZone::get("Europe/Helsinki").expect("time zone")
}

fn new_york() -> TimeZone {
    TimeZone::get("America/New_York").expect("time zone")
}

// 2026-11-26 is a Thursday; Helsinki is UTC+2 in November.
const THURSDAY_NOON: &str = "2026-11-26T10:00:00Z";
const THURSDAY: Date = date(2026, 11, 26);

#[test]
fn a_weekday_morning_is_in_the_window() {
    assert!(in_window(Stock, at("2026-11-26T06:00:00Z"), &helsinki()));
}

#[test]
fn the_window_opens_at_seven_and_closes_at_ten_in_the_evening() {
    let edges = [
        in_window(Stock, at("2026-11-26T04:59:59Z"), &helsinki()),
        in_window(Stock, at("2026-11-26T05:00:00Z"), &helsinki()),
        in_window(Stock, at("2026-11-26T19:59:59Z"), &helsinki()),
        in_window(Stock, at("2026-11-26T20:00:00Z"), &helsinki()),
    ];

    assert_eq!(edges, [false, true, true, false]);
}

#[test]
fn a_saturday_is_outside_the_window() {
    assert!(!in_window(Stock, at("2026-11-28T10:00:00Z"), &helsinki()));
}

#[test]
fn crypto_is_always_in_the_window() {
    assert!(in_window(Crypto, at("2026-11-28T23:30:00Z"), &helsinki()));
}

#[test]
fn the_window_follows_the_exchange_time_zone() {
    let now = at("2026-11-26T06:30:00Z");

    let open = (
        in_window(Stock, now, &helsinki()),
        in_window(Stock, now, &new_york()),
    );

    assert_eq!(open, (true, false));
}

#[test]
fn a_quote_older_than_the_stale_after_in_the_window_is_stale() {
    let stale = is_stale(
        Stock,
        THURSDAY,
        at("2026-11-26T09:14:59Z"),
        at(THURSDAY_NOON),
        &helsinki(),
        true,
    );

    assert!(stale);
}

#[test]
fn outside_the_window_the_last_close_stays_fresh() {
    let friday_close = at("2026-11-27T15:30:00Z");

    let stale = is_stale(
        Stock,
        date(2026, 11, 27),
        friday_close,
        at("2026-11-28T10:00:00Z"),
        &helsinki(),
        true,
    );

    assert!(!stale);
}

#[test]
fn a_nav_three_business_days_old_is_fresh_and_four_is_stale() {
    let now = at(THURSDAY_NOON);
    let monday = date(2026, 11, 23);
    let friday = date(2026, 11, 20);

    let verdicts = (
        is_stale(Fund, monday, now, now, &helsinki(), false),
        is_stale(Fund, friday, now, now, &helsinki(), false),
    );

    assert_eq!(verdicts, (false, true));
}

#[test]
fn a_fund_is_not_stale_by_the_intraday_rule() {
    let two_days_ago = at("2026-11-24T10:00:00Z");

    let stale = is_stale(
        Fund,
        date(2026, 11, 25),
        two_days_ago,
        at(THURSDAY_NOON),
        &helsinki(),
        false,
    );

    assert!(!stale);
}

#[test]
fn an_old_stock_price_is_stale_even_outside_the_window() {
    let stale = is_stale(
        Stock,
        date(2026, 11, 20),
        at("2026-11-20T15:00:00Z"),
        at("2026-11-28T10:00:00Z"),
        &helsinki(),
        true,
    );

    assert!(stale);
}

#[test]
fn business_days_skip_weekends() {
    let counts = (
        business_days_after(date(2026, 11, 27), date(2026, 11, 30)),
        business_days_after(THURSDAY, THURSDAY),
        business_days_after(THURSDAY, date(2026, 11, 20)),
        business_days_after(date(2026, 11, 20), THURSDAY),
    );

    assert_eq!(counts, (1, 0, 0, 4));
}

#[test]
fn a_quote_fifteen_minutes_behind_the_market_is_stale() {
    let stale = is_stale(
        Stock,
        THURSDAY,
        at("2026-11-26T09:44:59Z"),
        at(THURSDAY_NOON),
        &helsinki(),
        true,
    );

    assert!(stale);
}

#[test]
fn a_quote_ten_minutes_behind_the_market_is_fresh() {
    let stale = is_stale(
        Stock,
        THURSDAY,
        at("2026-11-26T09:50:00Z"),
        at(THURSDAY_NOON),
        &helsinki(),
        true,
    );

    assert!(!stale);
}

#[test]
fn a_fund_does_not_fall_due_before_the_slot() {
    let morning = at("2026-11-26T14:00:00Z");

    assert!(!fund_due(morning, None, &helsinki()));
}

#[test]
fn a_fund_falls_due_after_the_slot_until_it_is_fetched() {
    let evening = at("2026-11-26T18:30:00Z");
    let already = at("2026-11-26T18:10:00Z");
    let before_slot = at("2026-11-26T15:00:00Z");
    let yesterday = at("2026-11-25T18:10:00Z");

    let verdicts = (
        fund_due(evening, None, &helsinki()),
        fund_due(evening, Some(already), &helsinki()),
        fund_due(evening, Some(before_slot), &helsinki()),
        fund_due(evening, Some(yesterday), &helsinki()),
    );

    assert_eq!(verdicts, (true, false, true, true));
}

#[test]
fn a_fund_does_not_fall_due_at_the_weekend() {
    let saturday_evening = at("2026-11-28T18:30:00Z");

    assert!(!fund_due(saturday_evening, None, &helsinki()));
}
