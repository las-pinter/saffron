//! Prices over HTTP: the scheduled refresh, the FT fund fallback, the stale flag,
//! manual entry, and the settings that switch the fallback on.

mod common;

use axum::http::{Method, StatusCode};
use common::instrument_steps::{Market, add, market};
use jiff::{SignedDuration, civil::date};
use saffron_db::prices::{self, Price};
use saffron_server::refresh;
use serde_json::json;

async fn set_ft_fallback(market: &Market, on: bool) -> (StatusCode, serde_json::Value) {
    let reply = market
        .app
        .call(Method::PATCH, "/api/settings")
        .cookie(&market.admin)
        .json(json!({ "ft_fallback": on }))
        .send()
        .await;
    (reply.status, reply.json())
}

/// A fund added through the API, with its Yahoo details from the fixtures.
async fn op_fund(market: &Market) -> i64 {
    add(
        market,
        json!({ "symbol": "0P0001DHRJ.F", "isin": "FI4000315700" }),
    )
    .await
    .json()["id"]
        .as_i64()
        .expect("instrument id")
}

async fn latest(market: &Market, id: i64) -> serde_json::Value {
    market
        .app
        .get(&format!("/api/instruments/{id}"))
        .cookie(&market.admin)
        .send()
        .await
        .json()["latest_price"]
        .clone()
}

#[tokio::test]
async fn a_failed_history_is_fetched_once_the_provider_returns() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    // The provider fails: a quote may still arrive, but the history cannot.
    market
        .yahoo
        .fail_yahoo_with(StatusCode::INTERNAL_SERVER_ERROR);
    market.app.advance(SignedDuration::from_hours(48));
    refresh::tick(&market.app.state).await.expect("failed tick");
    assert_eq!(
        prices::history(
            &market.app.state.pool,
            id.as_i64().expect("id"),
            date(2026, 1, 1)
        )
        .await
        .expect("history"),
        Vec::new(),
        "nothing was fetched while the provider failed"
    );

    // An hour later the provider answers again.
    market.app.advance(SignedDuration::from_mins(61));
    market.yahoo.recover();
    refresh::tick(&market.app.state)
        .await
        .expect("recovered tick");

    let fetched = prices::history(
        &market.app.state.pool,
        id.as_i64().expect("id"),
        date(2026, 1, 1),
    )
    .await
    .expect("history");
    assert_eq!(fetched.len(), 1, "the year of history is not lost");
}

#[tokio::test]
async fn a_failed_quote_waits_out_the_refresh_interval() {
    let market = market().await;
    add(&market, json!({ "symbol": "NOKIA.HE" })).await;
    // Two days on, so the recorded close is older than the refresh interval.
    market.app.advance(SignedDuration::from_hours(48));
    refresh::tick(&market.app.state)
        .await
        .expect("history first");
    market.app.advance(SignedDuration::from_mins(20));
    market
        .yahoo
        .fail_yahoo_with(StatusCode::INTERNAL_SERVER_ERROR);

    let before = market.yahoo.requests();
    let failed_tick = refresh::tick(&market.app.state).await.expect("refresh");
    let after_failed = market.yahoo.requests();
    let next_tick = refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        (
            failed_tick.failed,
            after_failed - before,
            market.yahoo.requests() - after_failed,
            next_tick.failed
        ),
        (1, 1, 0, 0),
        "one attempt, then nothing until the refresh interval passes"
    );
}

#[tokio::test]
async fn a_fund_quote_waits_for_the_evening_slot() {
    let market = market().await;
    add(&market, json!({ "symbol": "0P0001DHRJ.F" })).await;

    let morning = refresh::tick(&market.app.state).await.expect("refresh");
    market.app.advance(SignedDuration::from_hours(6));
    let evening = refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        (
            morning.quotes,
            evening.quotes,
            market.yahoo.spark_requests()
        ),
        (0, 0, 1),
        "asked once in the evening, never in the morning, with no close in the fixture"
    );
}

#[tokio::test]
async fn a_fund_with_no_price_is_asked_for_once_an_evening() {
    let market = market().await;
    add(&market, json!({ "symbol": "0P0001DHRJ.F" })).await;
    market.app.advance(SignedDuration::from_hours(6));

    let first = refresh::tick(&market.app.state)
        .await
        .expect("evening tick");
    market.app.advance(SignedDuration::from_mins(30));
    let second = refresh::tick(&market.app.state).await.expect("later tick");

    assert_eq!(
        (first.quotes, second.quotes, market.yahoo.spark_requests()),
        (0, 0, 1),
        "the evening slot is used once, though no price came back"
    );
}

#[tokio::test]
async fn a_new_instrument_gets_a_year_of_history() {
    let mut market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    // Saturday: no exchange is open, so only the history backfill can fill this in.
    market.app.advance(SignedDuration::from_hours(72));
    market.admin = market
        .app
        .login("admin", common::ADMIN_PASSWORD)
        .await
        .session_cookie()
        .expect("a fresh session after three days");

    refresh::tick(&market.app.state).await.expect("refresh");
    let history = market
        .app
        .get(&format!(
            "/api/instruments/{}/prices",
            id.as_i64().expect("id")
        ))
        .cookie(&market.admin)
        .send()
        .await
        .json();

    assert_eq!(
        history,
        json!([{ "date": "2026-11-26", "close": "6.639999866485596", "currency": "EUR", "source": "yahoo" }])
    );
}

#[tokio::test]
async fn a_new_instrument_gets_history_and_a_quote_on_a_trading_day() {
    let mut market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    market.app.advance(SignedDuration::from_hours(48));
    market.admin = market
        .app
        .login("admin", common::ADMIN_PASSWORD)
        .await
        .session_cookie()
        .expect("a fresh session");

    let done = refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        (done.history, done.quotes),
        (1, 1),
        "history is fetched even though the same tick also stores a quote"
    );
    assert!(latest(&market, id.as_i64().expect("id")).await["close"].is_string());
}

#[tokio::test]
async fn a_quote_older_than_the_fund_rule_sends_the_fund_to_ft() {
    let market = market().await;
    // A fund whose Yahoo price is a year old, stored the way the refresh would.
    let new = saffron_db::instruments::NewInstrument {
        kind: saffron_core::instrument::InstrumentKind::Fund,
        name: "OP-Stale Fund",
        isin: Some("FI4000315700"),
        currency: "EUR",
    };
    let listing = saffron_db::instruments::NewListing {
        provider: "yahoo",
        symbol: "STALE.F",
        exchange: Some("Frankfurt"),
        currency: "EUR",
        timezone: Some("Europe/Berlin"),
    };
    let now = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    let id = saffron_db::instruments::create(&market.app.state.pool, new, listing, now)
        .await
        .expect("create");
    set_ft_fallback(&market, true).await;
    // Yahoo answers, but with a market time from the year before.
    let ancient = "2025-11-25T12:00:00Z".parse().expect("timestamp");
    prices::record(
        &market.app.state.pool,
        &Price {
            instrument_id: id,
            date: date(2025, 11, 25),
            close: "100".parse().expect("decimal"),
            currency: "EUR".to_owned(),
            source: "yahoo".to_owned(),
            as_of: ancient,
            checked_at: ancient,
        },
    )
    .await
    .expect("record");
    market.app.advance(SignedDuration::from_hours(6));

    refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        latest(&market, id).await["source"].clone(),
        json!("ft"),
        "a stale Yahoo NAV gives way to a fresher FT one"
    );
}

#[tokio::test]
async fn an_old_listing_learns_its_exchange_time_zone() {
    let market = market().await;
    // A listing stored before time zones were saved.
    let listing = saffron_db::instruments::NewListing {
        provider: "yahoo",
        symbol: "NOKIA.HE",
        exchange: Some("HEL"),
        currency: "EUR",
        timezone: None,
    };
    let new = saffron_db::instruments::NewInstrument {
        kind: saffron_core::instrument::InstrumentKind::Stock,
        name: "Nokia",
        isin: None,
        currency: "EUR",
    };
    let now = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    saffron_db::instruments::create(&market.app.state.pool, new, listing, now)
        .await
        .expect("create");

    refresh::tick(&market.app.state).await.expect("refresh");

    let stored = saffron_db::instruments::get(&market.app.state.pool, 1)
        .await
        .expect("get")
        .expect("instrument");
    assert_eq!(
        stored.listings[0].timezone.as_deref(),
        Some("Europe/Helsinki")
    );
}

#[tokio::test]
async fn an_unknown_symbol_is_not_polled_for_its_time_zone_every_tick() {
    let market = market().await;
    // A listing Yahoo has nothing for: its details call answers 404.
    let new = saffron_db::instruments::NewInstrument {
        kind: saffron_core::instrument::InstrumentKind::Stock,
        name: "Ghost",
        isin: None,
        currency: "EUR",
    };
    let listing = saffron_db::instruments::NewListing {
        provider: "yahoo",
        symbol: "NOSUCH123XX",
        exchange: None,
        currency: "EUR",
        timezone: None,
    };
    let now = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    saffron_db::instruments::create(&market.app.state.pool, new, listing, now)
        .await
        .expect("create");

    refresh::tick(&market.app.state).await.expect("first tick");
    let asked = market.yahoo.requests();
    refresh::tick(&market.app.state).await.expect("second tick");

    assert_eq!(market.yahoo.requests() - asked, 0, "the lookup waits a day");
}
#[tokio::test]
async fn no_quote_is_fetched_while_the_exchange_is_closed() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    let id = id.as_i64().expect("id");
    refresh::tick(&market.app.state).await.expect("refresh");
    let wednesday = prices::latest(&market.app.state.pool, id)
        .await
        .expect("latest")
        .expect("a quote");
    // Saturday at 14:00 Helsinki.
    market.app.advance(SignedDuration::from_hours(72));

    let done = refresh::tick(&market.app.state).await.expect("refresh");

    let saturday = prices::latest(&market.app.state.pool, id)
        .await
        .expect("latest")
        .expect("the same quote");
    assert_eq!(
        (done.quotes, saturday.checked_at, wednesday.checked_at),
        (
            0,
            wednesday.checked_at,
            "2026-11-25T12:00:00Z".parse().expect("timestamp")
        )
    );
}

#[tokio::test]
async fn quotes_come_in_one_request_for_every_listing() {
    let mut market = market().await;
    let nokia = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    let vodafone = add(&market, json!({ "symbol": "VOD.L" })).await.json()["id"].clone();
    // Two days on, so the recorded closes are older than the refresh interval.
    market.app.advance(SignedDuration::from_hours(48));
    market.admin = market
        .app
        .login("admin", common::ADMIN_PASSWORD)
        .await
        .session_cookie()
        .expect("a fresh session");
    let done = refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        (done.quotes, market.yahoo.spark_requests()),
        (2, 1),
        "two quotes, one batched request"
    );
    assert_eq!(
        (
            latest(&market, nokia.as_i64().expect("id")).await["close"].clone(),
            latest(&market, vodafone.as_i64().expect("id")).await["close"].clone()
        ),
        (json!("6.639999866485596"), json!("1.0305000305175781")),
        "pence are stored in pounds"
    );
}

#[tokio::test]
async fn when_yahoo_fails_the_ft_fallback_supplies_the_nav() {
    let market = market().await;
    let id = op_fund(&market).await;
    set_ft_fallback(&market, true).await;
    // Funds are due after 20:00 Helsinki; the clock starts at 14:00.
    market.app.advance(SignedDuration::from_hours(6));
    market
        .yahoo
        .fail_yahoo_with(StatusCode::INTERNAL_SERVER_ERROR);

    let done = refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        (done.quotes, latest(&market, id).await),
        (
            1,
            json!({
                "date": "2026-11-25",
                "close": "136.87",
                "currency": "EUR",
                "source": "ft",
                "as_of": "2026-11-25T00:00:00Z",
                "checked_at": "2026-11-25T18:00:00Z",
                "stale": false,
            })
        )
    );
}

#[tokio::test]
async fn an_ft_price_in_another_currency_is_refused() {
    let market = market().await;
    market.yahoo.serve_two_currency_funds();
    let id = op_fund(&market).await;
    set_ft_fallback(&market, true).await;
    market.app.advance(SignedDuration::from_hours(6));
    market
        .yahoo
        .fail_yahoo_with(StatusCode::INTERNAL_SERVER_ERROR);

    refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(
        latest(&market, id).await,
        json!(null),
        "an EUR fund does not take a USD price"
    );
}
