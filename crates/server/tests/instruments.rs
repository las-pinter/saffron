//! Instrument lookup and adding over HTTP, against a stand-in Yahoo that serves
//! recorded responses.

mod common;

use axum::http::StatusCode;
use common::{
    USER_PASSWORD,
    instrument_steps::{add, instrument_count, lookup, market},
    portfolio_steps::error,
};
use serde_json::{Value, json};

#[tokio::test]
async fn an_op_fund_isin_resolves_to_its_yahoo_listing() {
    let market = market().await;

    let reply = lookup(&market, "FI4000315700").await;

    assert_eq!(
        reply.json(),
        json!({
            "isin": "FI4000315700",
            "results": [{
                "provider": "yahoo",
                "symbol": "0P0001DHRJ.F",
                "name": "OP-Suomi Indeksi B",
                "kind": "fund",
                "exchange": "Frankfurt",
                "instrument_id": null,
            }],
        })
    );
}

#[tokio::test]
async fn adding_the_op_fund_stores_it_in_euros() {
    let market = market().await;

    let reply = add(
        &market,
        json!({ "symbol": "0P0001DHRJ.F", "isin": "FI4000315700" }),
    )
    .await;

    assert_eq!(reply.status, StatusCode::CREATED);
    let mut body = reply.json();
    body["id"] = Value::Null;
    assert_eq!(
        body,
        json!({
            "id": null,
            "kind": "fund",
            "name": "OP-Suomi Indeksi B",
            "isin": "FI4000315700",
            "currency": "EUR",
            "asset_class": "equity",
            "region": "europe",
            "created_at": "2026-11-25T12:00:00Z",
            "latest_price": null,
            "listings": [{
                "provider": "yahoo",
                "symbol": "0P0001DHRJ.F",
                "exchange": "Frankfurt",
                "currency": "EUR",
                "is_primary": true,
            }],
        })
    );
}

#[tokio::test]
async fn a_lookup_marks_listings_saffron_already_has() {
    let market = market().await;
    let id = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json()["id"].clone();

    let results = lookup(&market, "nokia").await.json()["results"].clone();

    let already_known: Vec<_> = results
        .as_array()
        .expect("results")
        .iter()
        .filter(|hit| !hit["instrument_id"].is_null())
        .map(|hit| (hit["symbol"].clone(), hit["instrument_id"].clone()))
        .collect();
    assert_eq!(already_known, [(json!("NOKIA.HE"), id)]);
}

#[tokio::test]
async fn adding_a_known_listing_returns_the_existing_instrument() {
    let market = market().await;
    let first = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json();
    let asked = market.yahoo.requests();

    let again = add(&market, json!({ "symbol": "NOKIA.HE" })).await;

    assert_eq!(
        (
            again.status,
            again.json()["id"].clone(),
            instrument_count(&market).await,
            market.yahoo.requests()
        ),
        (StatusCode::OK, first["id"].clone(), 1, asked)
    );
}

#[tokio::test]
async fn a_symbol_in_lower_case_finds_the_same_listing() {
    let market = market().await;
    let first = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json();

    let again = add(&market, json!({ "symbol": "nokia.he" })).await;

    assert_eq!(
        (
            again.status,
            again.json()["id"].clone(),
            instrument_count(&market).await
        ),
        (StatusCode::OK, first["id"].clone(), 1)
    );
}

#[tokio::test]
async fn an_unknown_symbol_is_refused() {
    let market = market().await;

    let reply = add(&market, json!({ "symbol": "NOSUCH123XX" })).await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("unknown_symbol"))
    );
}

#[tokio::test]
async fn a_symbol_with_spaces_is_refused() {
    let market = market().await;

    let reply = add(&market, json!({ "symbol": "NOKIA HE" })).await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_symbol"))
    );
}

#[tokio::test]
async fn a_blank_lookup_is_refused() {
    let market = market().await;

    let reply = lookup(&market, "   ").await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_query"))
    );
}

#[tokio::test]
async fn a_lookup_needs_its_query() {
    let market = market().await;

    let reply = market
        .app
        .get("/api/instruments/lookup")
        .cookie(&market.admin)
        .send()
        .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_query"))
    );
}

#[tokio::test]
async fn a_rate_limited_provider_is_reported_as_unavailable() {
    let market = market().await;
    market.yahoo.fail_with(StatusCode::TOO_MANY_REQUESTS);

    let reply = lookup(&market, "nokia").await;

    assert_eq!(
        error(&reply),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!("provider_unavailable")
        )
    );
}

#[tokio::test]
async fn an_unreadable_provider_answer_is_a_bad_gateway() {
    let market = market().await;
    market.yahoo.fail_with(StatusCode::NO_CONTENT);

    let reply = add(&market, json!({ "symbol": "NOKIA.HE" })).await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_GATEWAY, json!("provider_error"))
    );
}

#[tokio::test]
async fn a_member_may_add_an_instrument() {
    let market = market().await;
    market.app.create_user(&market.admin, "member").await;
    let member = market
        .app
        .login("member", USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie");

    let reply = market
        .app
        .post("/api/instruments")
        .cookie(&member)
        .json(json!({ "symbol": "NOKIA.HE" }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::CREATED);
}

#[tokio::test]
async fn an_added_instrument_is_shown_by_id() {
    let market = market().await;
    let added = add(&market, json!({ "symbol": "NOKIA.HE" })).await.json();

    let shown = market
        .app
        .get(&format!("/api/instruments/{}", added["id"]))
        .cookie(&market.admin)
        .send()
        .await
        .json();

    assert_eq!(shown, added);
}

#[tokio::test]
async fn an_unknown_instrument_is_404() {
    let market = market().await;
    add(&market, json!({ "symbol": "NOKIA.HE" })).await;

    let reply = market
        .app
        .get("/api/instruments/999")
        .cookie(&market.admin)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn instruments_need_a_session() {
    let market = market().await;

    let found = market
        .app
        .get("/api/instruments/lookup?q=nokia")
        .send()
        .await;
    let added = market
        .app
        .post("/api/instruments")
        .json(json!({ "symbol": "NOKIA.HE" }))
        .send()
        .await;

    assert_eq!(
        (found.status, added.status, market.yahoo.requests()),
        (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED, 0)
    );
}

#[tokio::test]
async fn adding_an_instrument_is_audited() {
    let market = market().await;

    add(&market, json!({ "symbol": "0P0001DHRJ.F" })).await;

    let row: (String, String, String) =
        sqlx::query_as("SELECT action, target, detail FROM audit_log ORDER BY id DESC LIMIT 1")
            .fetch_one(&market.app.state.pool)
            .await
            .expect("audit");
    assert_eq!(
        row,
        (
            "instrument.create".to_owned(),
            "OP-Suomi Indeksi B".to_owned(),
            r#"{"symbol":"0P0001DHRJ.F"}"#.to_owned()
        )
    );
}

#[tokio::test]
async fn a_listing_quoted_in_pence_is_valued_in_pounds() {
    let market = market().await;

    let body = add(&market, json!({ "symbol": "VOD.L" })).await.json();

    assert_eq!(
        (
            body["currency"].clone(),
            body["listings"][0]["currency"].clone()
        ),
        (json!("GBP"), json!("GBX"))
    );
}

#[tokio::test]
async fn an_index_cannot_be_added() {
    let market = market().await;

    let reply = add(&market, json!({ "symbol": "^OMXH25" })).await;

    assert_eq!(
        (error(&reply), instrument_count(&market).await),
        ((StatusCode::BAD_REQUEST, json!("unsupported_kind")), 0)
    );
}

#[tokio::test]
async fn a_symbol_that_walks_the_path_is_refused() {
    let market = market().await;

    let replies = [
        add(&market, json!({ "symbol": ".." })).await,
        add(&market, json!({ "symbol": "a/b" })).await,
    ];

    assert_eq!(
        (
            error(&replies[0]),
            error(&replies[1]),
            market.yahoo.requests()
        ),
        (
            (StatusCode::BAD_REQUEST, json!("invalid_symbol")),
            (StatusCode::BAD_REQUEST, json!("invalid_symbol")),
            0
        )
    );
}
