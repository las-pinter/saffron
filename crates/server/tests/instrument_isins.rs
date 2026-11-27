//! Adding an instrument with an ISIN: the ISIN is checked against Yahoo, and a
//! listing joins the instrument that already has that ISIN.

mod common;

use axum::http::StatusCode;
use common::{
    instrument_steps::{add, instrument_count, market},
    portfolio_steps::error,
};
use jiff::Timestamp;
use saffron_core::instrument::InstrumentKind;
use saffron_db::instruments::{self, NewInstrument, NewListing};
use serde_json::json;

#[tokio::test]
async fn an_isin_that_does_not_list_the_symbol_is_refused() {
    let market = market().await;

    let reply = add(
        &market,
        json!({ "symbol": "NOKIA.HE", "isin": "FI4000315700" }),
    )
    .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("isin_mismatch"))
    );
    assert_eq!(instrument_count(&market).await, 0);
}

#[tokio::test]
async fn a_listing_with_the_isin_of_a_known_instrument_joins_it() {
    let market = market().await;
    let now: Timestamp = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    let etf = NewInstrument {
        kind: InstrumentKind::Etf,
        name: "iShares Core MSCI World",
        isin: Some("IE00B4L5Y983"),
        currency: "EUR",
    };
    let elsewhere = NewListing {
        provider: "ft",
        symbol: "IWDA:LSE:USD",
        exchange: None,
        currency: "EUR",
        timezone: None,
    };
    let id = instruments::create(&market.app.state.pool, etf, elsewhere, now)
        .await
        .expect("create");

    let reply = add(
        &market,
        json!({ "symbol": "IWDA.L", "isin": "IE00B4L5Y983" }),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    let listings: Vec<_> = body["listings"]
        .as_array()
        .expect("listings")
        .iter()
        .map(|listing| (listing["symbol"].clone(), listing["is_primary"].clone()))
        .collect();
    assert_eq!(
        (body["id"].clone(), listings),
        (
            json!(id),
            vec![
                (json!("IWDA:LSE:USD"), json!(true)),
                (json!("IWDA.L"), json!(false))
            ]
        )
    );
}

#[tokio::test]
async fn a_bad_isin_is_refused_without_asking_the_provider() {
    let market = market().await;

    let reply = add(
        &market,
        json!({ "symbol": "0P0001DHRJ.F", "isin": "FI4000315701" }),
    )
    .await;

    assert_eq!(
        (error(&reply), market.yahoo.requests()),
        ((StatusCode::BAD_REQUEST, json!("invalid_isin")), 0)
    );
}

#[tokio::test]
async fn a_joined_listing_keeps_its_own_currency() {
    let market = market().await;
    let now: Timestamp = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    let etf = NewInstrument {
        kind: InstrumentKind::Etf,
        name: "iShares Core MSCI World",
        isin: Some("IE00B4L5Y983"),
        currency: "EUR",
    };
    let amsterdam = NewListing {
        provider: "ft",
        symbol: "IWDA:AEX:EUR",
        exchange: None,
        currency: "EUR",
        timezone: None,
    };
    instruments::create(&market.app.state.pool, etf, amsterdam, now)
        .await
        .expect("create");

    let body = add(
        &market,
        json!({ "symbol": "IWDA.L", "isin": "IE00B4L5Y983" }),
    )
    .await
    .json();

    assert_eq!(
        (
            body["currency"].clone(),
            body["listings"][1]["currency"].clone()
        ),
        (json!("EUR"), json!("USD"))
    );
}

#[tokio::test]
async fn a_joined_listing_is_audited() {
    let market = market().await;
    let now: Timestamp = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    let fund = NewInstrument {
        kind: InstrumentKind::Fund,
        name: "OP-Suomi Indeksi B",
        isin: Some("FI4000315700"),
        currency: "EUR",
    };
    let elsewhere = NewListing {
        provider: "ft",
        symbol: "FI4000315700:EUR",
        exchange: None,
        currency: "EUR",
        timezone: None,
    };
    instruments::create(&market.app.state.pool, fund, elsewhere, now)
        .await
        .expect("create");

    add(
        &market,
        json!({ "symbol": "0P0001DHRJ.F", "isin": "FI4000315700" }),
    )
    .await;

    let row: (String, String) =
        sqlx::query_as("SELECT action, target FROM audit_log ORDER BY id DESC LIMIT 1")
            .fetch_one(&market.app.state.pool)
            .await
            .expect("audit");
    assert_eq!(
        row,
        (
            "instrument.add_listing".to_owned(),
            "OP-Suomi Indeksi B".to_owned()
        )
    );
}
