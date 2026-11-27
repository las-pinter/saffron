//! The FT client against recorded responses.

use std::str::FromStr;

use jiff::civil::date;
use reqwest::StatusCode;
use rust_decimal::Decimal;
use saffron_core::instrument::Isin;
use saffron_market::{
    DailyClose, MarketError, Quote,
    ft::{Ft, FtFund},
    mock::MockMarket,
};

fn isin(code: &str) -> Isin {
    Isin::parse(code).expect("isin")
}

fn nav(text: &str) -> Decimal {
    Decimal::from_str(text).expect("decimal")
}

fn op_suomi() -> FtFund {
    FtFund {
        symbol: "FI4000315700:EUR".to_owned(),
        xid: "118011809".to_owned(),
        name: "OP-Suomi Indeksi B".to_owned(),
        currency: "EUR".to_owned(),
    }
}

#[tokio::test]
async fn an_op_fund_is_found_by_isin() {
    let market = MockMarket::start().await;

    let fund = market
        .ft_client()
        .expect("client")
        .find_fund(&isin("FI4000315700"))
        .await;

    assert_eq!(fund.expect("fund"), op_suomi());
}

#[tokio::test]
async fn a_fuzzy_match_on_another_security_is_not_found() {
    let market = MockMarket::start().await;

    let fund = market
        .ft_client()
        .expect("client")
        .find_fund(&isin("FI0008801048"))
        .await;

    assert!(matches!(fund, Err(MarketError::NotFound)), "{fund:?}");
}

#[tokio::test]
async fn fund_history_is_read_oldest_first() {
    let market = MockMarket::start().await;

    let closes = market
        .ft_client()
        .expect("client")
        .history(&op_suomi(), date(2026, 11, 16), date(2026, 11, 26))
        .await
        .expect("history");

    assert_eq!(
        (
            closes.first().copied(),
            closes.last().copied(),
            closes.len()
        ),
        (
            Some(DailyClose {
                date: date(2026, 11, 16),
                close: nav("146.23")
            }),
            Some(DailyClose {
                date: date(2026, 11, 25),
                close: nav("136.87")
            }),
            8
        )
    );
}

#[tokio::test]
async fn the_latest_nav_is_the_newest_day() {
    let market = MockMarket::start().await;

    let quote = market
        .ft_client()
        .expect("client")
        .latest(&op_suomi(), date(2026, 11, 26))
        .await
        .expect("latest");

    assert_eq!(
        quote,
        Quote {
            symbol: "FI4000315700:EUR".to_owned(),
            date: date(2026, 11, 25),
            close: nav("136.87"),
            currency: "EUR".to_owned(),
            as_of: "2026-11-25T00:00:00Z".parse().expect("timestamp"),
            timezone: None,
        }
    );
}

#[tokio::test]
async fn a_fund_without_navs_has_no_latest() {
    let market = MockMarket::start().await;
    let unknown = FtFund {
        xid: "1".to_owned(),
        ..op_suomi()
    };

    let quote = market
        .ft_client()
        .expect("client")
        .latest(&unknown, date(2026, 11, 26))
        .await;

    assert!(matches!(quote, Err(MarketError::NotFound)), "{quote:?}");
}

#[tokio::test]
async fn a_429_from_ft_is_rate_limited() {
    let market = MockMarket::start().await;
    market.fail_with(StatusCode::TOO_MANY_REQUESTS);

    let fund = market
        .ft_client()
        .expect("client")
        .find_fund(&isin("FI4000315700"))
        .await;

    assert!(matches!(fund, Err(MarketError::RateLimited)), "{fund:?}");
}

#[tokio::test]
async fn live_endpoints_parse() {
    // Keeps the live URL constant honest without calling it.
    assert!(Ft::live().is_ok());
}

#[tokio::test]
async fn a_second_share_class_in_another_currency_is_reported_with_its_currency() {
    let market = MockMarket::start().await;
    market.serve_two_currency_funds();

    let fund = market
        .ft_client()
        .expect("client")
        .find_fund(&isin("FI4000315700"))
        .await
        .expect("fund");

    assert_eq!(
        (fund.symbol.as_str(), fund.currency.as_str()),
        ("FI4000315700:USD", "USD"),
        "the first class FT lists wins, and its currency travels with it"
    );
}
