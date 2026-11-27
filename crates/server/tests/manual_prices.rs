//! Manual prices over HTTP: entering a day by hand, and when a price is stale.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    instrument_steps::{Market, add, market},
    portfolio_steps::error,
};
use jiff::{SignedDuration, civil::date};
use saffron_db::prices::{self, Price};
use saffron_server::refresh;
use serde_json::json;

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
async fn a_manual_price_is_never_replaced_by_a_provider() {
    let mut market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    let id = id.as_i64().expect("id");
    let yesterday = SignedDuration::from_hours(24);
    market.app.advance(yesterday);
    market.admin = market
        .app
        .login("admin", common::ADMIN_PASSWORD)
        .await
        .session_cookie()
        .expect("a fresh session after a day");
    let entered = market
        .app
        .call(
            Method::PUT,
            &format!("/api/instruments/{id}/prices/2026-11-26"),
        )
        .cookie(&market.admin)
        .json(json!({ "close": "7.00" }))
        .send()
        .await;
    assert_eq!(entered.status, StatusCode::OK);
    // A quarter of an hour later the listing is due again, so a quote is fetched.
    market.app.advance(SignedDuration::from_mins(16));

    refresh::tick(&market.app.state).await.expect("refresh");

    let price = latest(&market, id).await;
    assert_eq!(
        (price["close"].clone(), price["source"].clone()),
        (json!("7"), json!("manual")),
        "the manual price stays, and the provider price for the day is dropped"
    );
}

#[tokio::test]
async fn a_price_entered_for_the_future_is_refused() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();

    let reply = market
        .app
        .call(
            Method::PUT,
            &format!("/api/instruments/{id}/prices/2026-11-30"),
        )
        .cookie(&market.admin)
        .json(json!({ "close": "6.5" }))
        .send()
        .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("future_date"))
    );
}

#[tokio::test]
async fn a_price_entered_today_is_not_stale_during_the_day() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();

    let entered = market
        .app
        .call(
            Method::PUT,
            &format!("/api/instruments/{id}/prices/2026-11-25"),
        )
        .cookie(&market.admin)
        .json(json!({ "close": "6.5" }))
        .send()
        .await;

    assert_eq!(
        entered.json()["stale"].clone(),
        json!(false),
        "a hand-entered price carries no market time"
    );
    assert_eq!(
        market
            .app
            .get(&format!("/api/instruments/{}", id.as_i64().expect("id")))
            .cookie(&market.admin)
            .send()
            .await
            .json()["latest_price"]["stale"]
            .clone(),
        json!(false)
    );
}

#[tokio::test]
async fn a_price_that_is_not_positive_is_refused() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();

    let replies = [
        market
            .app
            .call(
                Method::PUT,
                &format!("/api/instruments/{id}/prices/2026-11-20"),
            )
            .cookie(&market.admin)
            .json(json!({ "close": "0" }))
            .send()
            .await,
        market
            .app
            .call(
                Method::PUT,
                &format!("/api/instruments/{id}/prices/2026-11-20"),
            )
            .cookie(&market.admin)
            .json(json!({ "close": "-1" }))
            .send()
            .await,
        market
            .app
            .call(
                Method::PUT,
                &format!("/api/instruments/{id}/prices/2026-11-20"),
            )
            .cookie(&market.admin)
            .json(json!({ "close": "1e5" }))
            .send()
            .await,
    ];

    let errors: Vec<_> = replies.iter().map(error).collect();
    assert_eq!(
        errors,
        [
            (StatusCode::BAD_REQUEST, json!("invalid_price")),
            (StatusCode::BAD_REQUEST, json!("invalid_price")),
            (StatusCode::BAD_REQUEST, json!("invalid_price")),
        ]
    );
}

#[tokio::test]
async fn a_quote_confirmed_just_now_is_stale_when_the_market_time_is_old() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    let id = id.as_i64().expect("id");
    let now = "2026-11-25T12:00:00Z".parse().expect("timestamp");
    prices::record(
        &market.app.state.pool,
        &Price {
            instrument_id: id,
            date: date(2026, 11, 25),
            close: "6.5".parse().expect("decimal"),
            currency: "EUR".to_owned(),
            source: "yahoo".to_owned(),
            as_of: "2026-11-25T09:00:00Z".parse().expect("timestamp"),
            checked_at: now,
        },
    )
    .await
    .expect("record");

    let price = latest(&market, id).await;

    assert_eq!(
        price["stale"].clone(),
        json!(true),
        "the market time is three hours old, though the provider answered a moment ago"
    );
}

#[tokio::test]
async fn an_entered_price_is_audited() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();

    market
        .app
        .call(
            Method::PUT,
            &format!("/api/instruments/{id}/prices/2026-11-20"),
        )
        .cookie(&market.admin)
        .json(json!({ "close": "6.5" }))
        .send()
        .await;

    let row: (String, String) =
        sqlx::query_as("SELECT action, detail FROM audit_log ORDER BY id DESC LIMIT 1")
            .fetch_one(&market.app.state.pool)
            .await
            .expect("audit");
    assert_eq!(
        row,
        (
            "price.manual".to_owned(),
            r#"{"close":"6.5","date":"2026-11-20"}"#.to_owned()
        )
    );
}

#[tokio::test]
async fn an_old_price_is_flagged_stale() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();
    let id = id.as_i64().expect("id");
    let old = "2026-11-10T15:00:00Z".parse().expect("timestamp");
    prices::record(
        &market.app.state.pool,
        &Price {
            instrument_id: id,
            date: date(2026, 11, 10),
            close: "6.5".parse().expect("decimal"),
            currency: "EUR".to_owned(),
            source: "yahoo".to_owned(),
            as_of: old,
            checked_at: old,
        },
    )
    .await
    .expect("record");

    let price = latest(&market, id).await;

    assert_eq!(price["stale"].clone(), json!(true));
}

#[tokio::test]
async fn prices_need_a_session() {
    let market = market().await;
    let id = op_fund(&market).await;

    let reply = market
        .app
        .get(&format!("/api/instruments/{id}/prices"))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}
