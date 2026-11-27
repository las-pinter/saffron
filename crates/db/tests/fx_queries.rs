//! FX rates: storage, the last-published-day rule, and conversion through the euro.

use std::{collections::BTreeMap, str::FromStr};

use jiff::civil::date;
use rust_decimal::Decimal;
use saffron_db::{
    fx::{self, FxRate},
    open_in_memory,
};
use sqlx::SqlitePool;

fn amount(text: &str) -> Decimal {
    Decimal::from_str(text).expect("decimal")
}

fn day(text: &str, usd: &str, gbp: &str, sek: &str) -> Vec<FxRate> {
    let date = text.parse().expect("date");
    [("USD", usd), ("GBP", gbp), ("SEK", sek)]
        .into_iter()
        .map(|(currency, per_eur)| FxRate {
            date,
            currency: currency.to_owned(),
            per_eur: amount(per_eur),
        })
        .collect()
}

async fn pool_with_rates() -> SqlitePool {
    let pool = open_in_memory().await.expect("database");
    let mut rates = day("2026-11-26", "1.0975", "0.83755", "11.2025");
    rates.extend(day("2026-11-25", "1.0935", "0.8373", "11.2385"));
    fx::upsert(&pool, &rates).await.expect("upsert");
    pool
}

#[tokio::test]
async fn a_usd_amount_converts_with_the_rate_of_its_date() {
    let pool = pool_with_rates().await;

    let converted = fx::convert(&pool, amount("100"), "USD", "EUR", date(2026, 11, 26))
        .await
        .expect("convert");

    assert_eq!(
        converted.map(|value| value.round_dp(2).to_string()),
        Some("91.12".to_owned()),
        "100 USD at 1.0975 per euro"
    );
}

#[tokio::test]
async fn a_price_between_two_currencies_goes_through_the_euro() {
    let pool = pool_with_rates().await;

    let converted = fx::convert(&pool, amount("100"), "USD", "GBP", date(2026, 11, 25))
        .await
        .expect("convert");

    // 100 / 1.0935 * 0.8373 on the 25th.
    assert_eq!(
        converted.map(|value| value.round_dp(4).to_string()),
        Some("76.5706".to_owned())
    );
}

#[tokio::test]
async fn a_range_starting_before_the_archive_keeps_its_rates() {
    let pool = pool_with_rates().await;

    let days = fx::between_with_prior(&pool, date(2026, 1, 1), date(2026, 11, 26))
        .await
        .expect("days");

    assert_eq!(
        days.iter().map(|(day, _)| *day).collect::<Vec<_>>(),
        [date(2026, 11, 25), date(2026, 11, 26)],
        "no day precedes the archive, so the range starts with its first day"
    );
}

#[tokio::test]
async fn a_prior_day_leads_the_range() {
    let pool = pool_with_rates().await;

    let days = fx::between_with_prior(&pool, date(2026, 11, 27), date(2026, 11, 28))
        .await
        .expect("days");

    assert_eq!(
        days.iter().map(|(day, _)| *day).collect::<Vec<_>>(),
        [date(2026, 11, 26)],
        "a start without a publication takes the last day before it"
    );
}

#[tokio::test]
async fn a_weekend_uses_the_last_published_day() {
    let pool = pool_with_rates().await;

    let saturday = fx::convert(&pool, amount("100"), "USD", "EUR", date(2026, 11, 28))
        .await
        .expect("convert");

    assert_eq!(
        saturday.map(|value| value.round_dp(2).to_string()),
        Some("91.12".to_owned()),
        "Thursday's rate stands in for the weekend"
    );
}

#[tokio::test]
async fn a_date_before_the_first_rate_converts_to_nothing() {
    let pool = pool_with_rates().await;

    let converted = fx::convert(&pool, amount("100"), "USD", "EUR", date(2026, 11, 1))
        .await
        .expect("convert");

    assert_eq!(converted, None);
}

#[tokio::test]
async fn a_missing_currency_converts_to_nothing() {
    let pool = pool_with_rates().await;

    let converted = fx::convert(&pool, amount("100"), "JPY", "EUR", date(2026, 11, 26))
        .await
        .expect("convert");

    assert_eq!(converted, None, "no rate, no invented number");
}

#[tokio::test]
async fn the_same_currency_converts_to_itself() {
    let pool = pool_with_rates().await;

    let converted = fx::convert(&pool, amount("42.5"), "EUR", "EUR", date(2026, 11, 26))
        .await
        .expect("convert");

    assert_eq!(converted, Some(amount("42.5")));
}

#[tokio::test]
async fn a_stored_rate_replaces_an_earlier_one_for_the_day() {
    let pool = pool_with_rates().await;
    let corrected = fx::FxRate {
        per_eur: amount("1.2"),
        ..day("2026-11-26", "1.0975", "0.83755", "11.2025").remove(0)
    };

    fx::upsert(&pool, &[corrected]).await.expect("upsert");

    let stored = fx::day_on_or_before(&pool, date(2026, 11, 26))
        .await
        .expect("day")
        .expect("rates");
    assert_eq!(stored.get("USD"), Some(&amount("1.2")));
}

#[tokio::test]
async fn the_latest_date_is_the_newest_published_day() {
    let pool = pool_with_rates().await;

    assert_eq!(
        fx::latest_date(&pool).await.expect("latest"),
        Some(date(2026, 11, 26))
    );
}

#[tokio::test]
async fn conversion_without_any_rates_is_none() {
    let pool = open_in_memory().await.expect("database");
    let empty: BTreeMap<String, Decimal> = BTreeMap::new();

    let (stored, converted) = (
        fx::day_on_or_before(&pool, date(2026, 11, 26))
            .await
            .expect("day"),
        fx::convert_with(&empty, amount("1"), "USD", "EUR"),
    );

    assert_eq!((stored, converted), (None, None));
}
