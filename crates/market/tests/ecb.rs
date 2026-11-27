//! The ECB client: the daily file and the history archive.

use std::str::FromStr;

use reqwest::StatusCode;
use rust_decimal::Decimal;
use saffron_market::{MarketError, mock::MockMarket};

fn rate(text: &str) -> Decimal {
    Decimal::from_str(text).expect("decimal")
}

#[tokio::test]
async fn the_daily_file_gives_todays_rates_per_euro() {
    let market = MockMarket::start().await;

    let rates = market
        .ecb_client()
        .expect("client")
        .daily()
        .await
        .expect("daily");

    assert_eq!(
        (rates.date.to_string(), rates.per_eur.get("USD").copied()),
        ("2026-11-26".to_owned(), Some(rate("1.0975")))
    );
}

#[tokio::test]
async fn the_history_archive_is_read_newest_first() {
    let market = MockMarket::start().await;

    let days = market
        .ecb_client()
        .expect("client")
        .history()
        .await
        .expect("history");

    assert_eq!(
        (
            days.len(),
            days.first().map(|day| day.date.to_string()),
            days.last().map(|day| day.date.to_string())
        ),
        (
            19,
            Some("2026-11-26".to_owned()),
            Some("2026-11-02".to_owned())
        )
    );
}

#[tokio::test]
async fn a_missing_currency_is_left_out_rather_than_read_as_zero() {
    let market = MockMarket::start().await;

    let rates = market
        .ecb_client()
        .expect("client")
        .daily()
        .await
        .expect("daily");

    assert!(
        !rates.per_eur.contains_key("N/A"),
        "only real three-letter codes are kept"
    );
    assert!(rates.per_eur.values().all(|rate| *rate > Decimal::ZERO));
}

#[tokio::test]
async fn a_refused_client_is_rate_limited() {
    let market = MockMarket::start().await;
    market.fail_with(StatusCode::TOO_MANY_REQUESTS);

    let rates = market.ecb_client().expect("client").daily().await;

    assert!(matches!(rates, Err(MarketError::RateLimited)), "{rates:?}");
}
