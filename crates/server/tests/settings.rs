//! The installation settings, and the FT fallback they switch on.

mod common;

use axum::http::{Method, StatusCode};
use common::instrument_steps::{Market, add, market};
use jiff::SignedDuration;
use saffron_server::refresh;
use serde_json::json;

#[allow(dead_code)]
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
async fn a_fallback_price_needs_the_setting_and_an_administrator() {
    let market = market().await;
    market.app.create_user(&market.admin, "member").await;
    let member = common::instrument_steps::member_cookie(&market).await;

    let refused = market
        .app
        .call(Method::PATCH, "/api/settings")
        .cookie(&member)
        .json(json!({ "ft_fallback": true }))
        .send()
        .await;
    let shown = market
        .app
        .get("/api/settings")
        .cookie(&market.admin)
        .send()
        .await;

    assert_eq!(
        (refused.status, shown.json()),
        (StatusCode::FORBIDDEN, json!({ "ft_fallback": false }))
    );
}

#[tokio::test]
async fn without_the_setting_the_fallback_stays_off() {
    let market = market().await;
    let id = op_fund(&market).await;
    // Funds are due after 20:00 Helsinki; the clock starts at 14:00.
    market.app.advance(SignedDuration::from_hours(6));
    market
        .yahoo
        .fail_yahoo_with(StatusCode::INTERNAL_SERVER_ERROR);

    refresh::tick(&market.app.state).await.expect("refresh");

    assert_eq!(latest(&market, id).await, json!(null));
}
