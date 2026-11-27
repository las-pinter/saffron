//! Performance over HTTP: the value series, TWR and XIRR, the benchmark, the
//! allocation with its targets, and who may read or change them.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    TestApp, book_steps,
    portfolio_steps::{household, portfolio, share},
};
use jiff::Timestamp;
use rust_decimal::Decimal;
use saffron_core::instrument::InstrumentKind;
use saffron_db::{
    fx::{self, FxRate},
    instruments::{self, NewInstrument, NewListing},
    prices::{self, Price},
};
use serde_json::{Value, json};

const CLOCK_DAY: &str = "2026-11-25";

struct Fixture {
    app: TestApp,
    cookie: String,
    portfolio: Value,
    account: i64,
    stock: i64,
}

/// A portfolio with 1000 EUR deposited, 10 shares bought at 50 EUR, and prices at
/// 50, 55, and 60 EUR. Its TWR over 2026-01-02 to 2026-02-10 is 10 percent.
async fn fixture() -> Fixture {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let portfolio = portfolio(&app, &cookie, "Home").await;
    let account = book_steps::account(
        &app,
        &cookie,
        &portfolio,
        json!({ "name": "Book", "kind": "book_entry" }),
    )
    .await;
    let stock = book_steps::instrument(&app, "Nokia", InstrumentKind::Stock, "EUR").await;
    book_steps::record(
        &app,
        &cookie,
        &portfolio,
        book_steps::deposit(account, "2026-01-02", "1000"),
    )
    .await;
    book_steps::record(
        &app,
        &cookie,
        &portfolio,
        json!({
            "account_id": account, "instrument_id": stock, "kind": "buy",
            "date": "2026-01-05", "quantity": "10", "price": "50",
        }),
    )
    .await;
    add_prices(
        &app,
        stock,
        "EUR",
        &[
            ("2026-01-05", "50"),
            ("2026-01-20", "55"),
            ("2026-02-01", "60"),
        ],
    )
    .await;
    Fixture {
        app,
        cookie,
        portfolio,
        account,
        stock,
    }
}

async fn add_prices(app: &TestApp, instrument: i64, currency: &str, closes: &[(&str, &str)]) {
    let at = "2026-01-01T00:00:00Z"
        .parse::<Timestamp>()
        .expect("timestamp");
    for (date, close) in closes {
        let stored = prices::record(
            &app.state.pool,
            &Price {
                instrument_id: instrument,
                date: date.parse().expect("date"),
                close: close.parse().expect("decimal"),
                currency: currency.to_owned(),
                source: "test".to_owned(),
                as_of: at,
                checked_at: at,
            },
        )
        .await
        .expect("price");
        assert!(stored, "a test price is stored");
    }
}

async fn add_fx(app: &TestApp, rates: &[(&str, &str)]) {
    let entries: Vec<FxRate> = rates
        .iter()
        .map(|(date, per_eur)| FxRate {
            date: date.parse().expect("date"),
            currency: "USD".to_owned(),
            per_eur: per_eur.parse().expect("decimal"),
        })
        .collect();
    fx::upsert(&app.state.pool, &entries).await.expect("rates");
}

async fn instrument_with_isin(
    app: &TestApp,
    symbol: &str,
    isin: &str,
    kind: InstrumentKind,
    currency: &str,
) -> i64 {
    let now = "2026-01-01T00:00:00Z"
        .parse::<Timestamp>()
        .expect("timestamp");
    instruments::create(
        &app.state.pool,
        NewInstrument {
            kind,
            name: symbol,
            isin: Some(isin),
            currency,
        },
        NewListing {
            provider: "test",
            symbol,
            exchange: None,
            currency,
            timezone: None,
        },
        now,
    )
    .await
    .expect("instrument")
}

async fn performance(f: &Fixture, query: &str) -> Value {
    let reply = f
        .app
        .get(&format!(
            "/api/portfolios/{}/performance{query}",
            f.portfolio
        ))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
    reply.json()
}

fn dec(value: &Value) -> Decimal {
    value
        .as_str()
        .expect("decimal text")
        .parse()
        .expect("a decimal")
}

fn near(value: &Value, expected: &str) -> bool {
    (dec(value) - expected.parse::<Decimal>().expect("expected")).abs()
        <= "0.01".parse::<Decimal>().expect("tolerance")
}

fn share_of<'a>(shares: &'a Value, key: &str) -> &'a Value {
    shares
        .as_array()
        .expect("shares are a list")
        .iter()
        .find(|share| share["key"] == json!(key))
        .expect("the share")
}

async fn set_benchmark(f: &Fixture, body: Value) -> common::Reply {
    f.app
        .call(
            Method::PUT,
            &format!("/api/portfolios/{}/benchmark", f.portfolio),
        )
        .cookie(&f.cookie)
        .json(body)
        .send()
        .await
}

async fn classify(f: &Fixture, instrument: i64, body: Value) -> common::Reply {
    f.app
        .call(
            Method::PUT,
            &format!("/api/instruments/{instrument}/classification"),
        )
        .cookie(&f.cookie)
        .json(body)
        .send()
        .await
}

async fn set_targets(f: &Fixture, body: Value) -> common::Reply {
    f.app
        .call(
            Method::PUT,
            &format!("/api/portfolios/{}/targets", f.portfolio),
        )
        .cookie(&f.cookie)
        .json(body)
        .send()
        .await
}

#[test]
fn classifications_have_sensible_defaults() {
    use saffron_server::performance::{default_asset_class, default_region};

    assert_eq!(default_asset_class(InstrumentKind::Stock), "equity");
    assert_eq!(default_asset_class(InstrumentKind::Bond), "bond");
    assert_eq!(default_asset_class(InstrumentKind::Crypto), "crypto");
    assert_eq!(default_region(Some("NOKIA.HE")), "europe");
    assert_eq!(default_region(Some("AAPL")), "north_america");
    assert_eq!(default_region(Some("7203.T")), "asia");
    assert_eq!(default_region(Some("X.ZZ")), "other");
    assert_eq!(default_region(None), "other");
}

#[tokio::test]
async fn reports_the_value_series_and_returns() {
    let f = fixture().await;

    let view = performance(&f, "?from=2026-01-02&to=2026-02-10").await;

    assert_eq!(view["from"], json!("2026-01-02"));
    assert_eq!(view["to"], json!("2026-02-10"));
    assert_eq!(view["base_currency"], json!("EUR"));
    let points = view["points"].as_array().expect("points");
    assert_eq!(points.len(), 40);
    assert_eq!(points[0]["value"], json!("1000"), "the deposit's day");
    assert_eq!(points[3]["value"], json!("1000"), "the buy's day");
    assert_eq!(points[18]["value"], json!("1050"));
    assert_eq!(points[39]["value"], json!("1100"));
    assert!(
        near(&view["twr_percent"], "10"),
        "{:?}",
        view["twr_percent"]
    );
    assert!(
        near(&view["xirr_percent"], "144.001827"),
        "{:?}",
        view["xirr_percent"]
    );
    assert_eq!(view["unpriced_instruments"], json!([]));
    assert_eq!(view["unpriced_currencies"], json!([]));
}

#[tokio::test]
async fn the_default_benchmark_is_compared_and_can_be_replaced() {
    let f = fixture().await;
    let default = instrument_with_isin(
        &f.app,
        "IWDA.AS",
        "IE00B4L5Y983",
        InstrumentKind::Etf,
        "EUR",
    )
    .await;
    add_prices(
        &f.app,
        default,
        "EUR",
        &[("2026-01-05", "100"), ("2026-02-01", "110")],
    )
    .await;
    let other = book_steps::instrument(&f.app, "BENCH.HE", InstrumentKind::Etf, "EUR").await;
    add_prices(
        &f.app,
        other,
        "EUR",
        &[("2026-01-05", "200"), ("2026-02-01", "180")],
    )
    .await;

    let view = performance(&f, "?from=2026-01-05&to=2026-02-10").await;
    assert_eq!(view["benchmark"]["instrument_id"], json!(default));
    assert!(near(&view["benchmark_percent"], "10"));

    let chosen = set_benchmark(&f, json!({ "instrument_id": other })).await;
    assert_eq!(chosen.status, StatusCode::OK, "{:?}", chosen.json());
    assert_eq!(chosen.json()["benchmark_instrument_id"], json!(other));

    let view = performance(&f, "?from=2026-01-05&to=2026-02-10").await;
    assert_eq!(view["benchmark"]["instrument_id"], json!(other));
    assert!(near(&view["benchmark_percent"], "-10"));

    let cleared = set_benchmark(&f, json!({ "instrument_id": null })).await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert_eq!(cleared.json()["benchmark_instrument_id"], json!(null));
    let view = performance(&f, "?from=2026-01-05&to=2026-02-10").await;
    assert_eq!(view["benchmark"]["instrument_id"], json!(default));

    let unknown = set_benchmark(&f, json!({ "instrument_id": 999_999 })).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown.json()["error"], json!("unknown_instrument"));
}

#[tokio::test]
async fn classification_is_stored_and_drives_the_allocation() {
    let f = fixture().await;

    let shown = f
        .app
        .get(&format!("/api/instruments/{}", f.stock))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(shown.status, StatusCode::OK);
    assert_eq!(shown.json()["asset_class"], json!("equity"));
    assert_eq!(shown.json()["region"], json!("north_america"));

    let reply = classify(
        &f,
        f.stock,
        json!({ "asset_class": "Bond", "region": " Europe " }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
    assert_eq!(reply.json()["asset_class"], json!("bond"));
    assert_eq!(reply.json()["region"], json!("europe"));

    let view = performance(&f, "?from=2026-01-02&to=2026-02-10").await;
    assert_eq!(
        share_of(&view["allocation"]["asset_class"], "bond")["value"],
        json!("600")
    );
    assert_eq!(
        share_of(&view["allocation"]["region"], "europe")["value"],
        json!("600")
    );
    assert!(
        view["allocation"]["asset_class"]
            .as_array()
            .expect("shares")
            .iter()
            .all(|share| share["key"] != json!("equity"))
    );

    let reply = classify(
        &f,
        f.stock,
        json!({ "asset_class": "equity", "region": "europe" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["asset_class"], json!("equity"));

    for body in [
        json!({ "asset_class": "gold", "region": "europe" }),
        json!({ "asset_class": "equity", "region": "moon" }),
    ] {
        let refused = classify(&f, f.stock, body).await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{:?}",
            refused.json()
        );
        assert_eq!(refused.json()["error"], json!("invalid_classification"));
    }
    let missing = classify(
        &f,
        999_999,
        json!({ "asset_class": "equity", "region": "europe" }),
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn targets_are_validated_and_show_drift() {
    let f = fixture().await;

    let stored = set_targets(
        &f,
        json!({ "asset_class": { "equity": "60", "cash": "40" } }),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK, "{:?}", stored.json());
    assert_eq!(
        stored.json(),
        json!({ "asset_class": { "cash": "40", "equity": "60" }, "region": {} })
    );

    let view = performance(&f, "?from=2026-01-02&to=2026-02-10").await;
    let equity = share_of(&view["allocation"]["asset_class"], "equity");
    assert_eq!(equity["target_percent"], json!("60"));
    assert!(near(&equity["value"], "600"));
    let weight = dec(&equity["weight_percent"]);
    let target: Decimal = "60".parse().expect("target");
    let drift = dec(&equity["drift_pp"]);
    assert!(
        (drift - (weight - target)).abs() <= "0.0001".parse::<Decimal>().expect("tolerance"),
        "drift {drift} against weight {weight}"
    );
    let cash = share_of(&view["allocation"]["asset_class"], "cash");
    assert_eq!(cash["target_percent"], json!("40"));

    for (body, code) in [
        (
            json!({ "asset_class": { "gold": "10" } }),
            json!("invalid_key"),
        ),
        (
            json!({ "asset_class": { "equity": "101" } }),
            json!("invalid_weight"),
        ),
        (
            json!({ "asset_class": { "equity": "-1" } }),
            json!("invalid_weight"),
        ),
        (
            json!({ "asset_class": { "equity": "60", "cash": "60" } }),
            json!("weights_over_100"),
        ),
    ] {
        let refused = set_targets(&f, body).await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{:?}",
            refused.json()
        );
        assert_eq!(refused.json()["error"], code);
    }

    let unchanged = f
        .app
        .get(&format!("/api/portfolios/{}/targets", f.portfolio))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(unchanged.json()["asset_class"]["equity"], json!("60"));
}

#[tokio::test]
async fn a_holding_without_a_price_is_named_not_counted() {
    let f = fixture().await;
    let unpriced = book_steps::instrument(&f.app, "Silent", InstrumentKind::Stock, "EUR").await;
    book_steps::record(
        &f.app,
        &f.cookie,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": unpriced, "kind": "buy",
            "date": "2026-01-06", "quantity": "5", "price": "10",
        }),
    )
    .await;

    let view = performance(&f, "?from=2026-02-10&to=2026-02-10").await;

    // The cash is 1000 - 500 - 50 = 450; the priced holding is 600 and the unpriced
    // one counts as zero.
    assert_eq!(view["points"][0]["value"], json!("1050"));
    assert_eq!(view["unpriced_instruments"], json!([unpriced]));
}

#[tokio::test]
async fn a_usd_account_is_valued_at_the_ecb_rate() {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let portfolio = portfolio(&app, &cookie, "Home").await;
    let account = book_steps::account(
        &app,
        &cookie,
        &portfolio,
        json!({ "name": "Dollars", "kind": "book_entry", "currency": "USD" }),
    )
    .await;
    book_steps::record(
        &app,
        &cookie,
        &portfolio,
        book_steps::deposit(account, "2026-01-02", "500"),
    )
    .await;
    add_fx(&app, &[("2026-01-02", "1.10")]).await;
    let f = Fixture {
        app,
        cookie,
        portfolio,
        account,
        stock: 0,
    };

    let view = performance(&f, "?from=2026-01-02&to=2026-01-02").await;

    let expected: Decimal =
        "500".parse::<Decimal>().expect("amount") / "1.10".parse::<Decimal>().expect("rate");
    assert_eq!(view["points"][0]["value"], json!(expected.to_string()));
    assert_eq!(view["points"][0]["flow"], json!(expected.to_string()));
}

#[tokio::test]
async fn ranges_are_validated_and_access_is_enforced() {
    let h = household().await;
    let id = portfolio(&h.app, &h.admin, "Home").await;
    let base = format!("/api/portfolios/{id}");

    let empty = h
        .app
        .get(&format!("{base}/performance"))
        .cookie(&h.admin)
        .send()
        .await;
    assert_eq!(empty.status, StatusCode::OK, "{:?}", empty.json());
    assert_eq!(empty.json()["from"], json!(CLOCK_DAY));
    assert_eq!(empty.json()["to"], json!(CLOCK_DAY));
    assert_eq!(empty.json()["points"].as_array().expect("points").len(), 1);
    assert_eq!(empty.json()["twr_percent"], json!(null));
    assert_eq!(empty.json()["xirr_percent"], json!(null));

    for (query, code) in [
        ("?from=2026-02-01&to=2026-01-01", "invalid_range"),
        ("?from=yesterday", "invalid_date"),
        ("?from=2000-01-01&to=2026-11-25", "range_too_long"),
    ] {
        let refused = h
            .app
            .get(&format!("{base}/performance{query}"))
            .cookie(&h.admin)
            .send()
            .await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST);
        assert_eq!(refused.json()["error"], json!(code));
    }

    let stranger = h
        .app
        .get(&format!("{base}/performance"))
        .cookie(&h.member)
        .send()
        .await;
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);

    share(&h.app, &h.admin, &id, "member", "viewer").await;
    let viewer = h
        .app
        .get(&format!("{base}/performance"))
        .cookie(&h.member)
        .send()
        .await;
    assert_eq!(viewer.status, StatusCode::OK);
    for (method, path, body) in [
        (Method::PUT, format!("{base}/targets"), json!({})),
        (Method::PUT, format!("{base}/benchmark"), json!({})),
    ] {
        let refused = h
            .app
            .call(method, &path)
            .cookie(&h.member)
            .json(body)
            .send()
            .await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "{path}");
    }

    share(&h.app, &h.admin, &id, "member", "editor").await;
    for (path, body) in [
        (
            format!("{base}/targets"),
            json!({ "asset_class": { "equity": "100" } }),
        ),
        (
            format!("{base}/benchmark"),
            json!({ "instrument_id": null }),
        ),
    ] {
        let allowed = h
            .app
            .call(Method::PUT, &path)
            .cookie(&h.member)
            .json(body)
            .send()
            .await;
        assert_eq!(
            allowed.status,
            StatusCode::OK,
            "{path}: {:?}",
            allowed.json()
        );
    }
}

#[tokio::test]
async fn a_future_transaction_does_not_break_the_default_range() {
    let h = household().await;
    let future = portfolio(&h.app, &h.admin, "Future").await;
    let account = book_steps::account(
        &h.app,
        &h.admin,
        &future,
        json!({ "name": "Book", "kind": "book_entry" }),
    )
    .await;
    book_steps::record(
        &h.app,
        &h.admin,
        &future,
        book_steps::deposit(account, "2026-12-01", "100"),
    )
    .await;

    let ahead = h
        .app
        .get(&format!("/api/portfolios/{future}/performance"))
        .cookie(&h.admin)
        .send()
        .await;

    assert_eq!(ahead.status, StatusCode::OK, "{:?}", ahead.json());
    assert_eq!(ahead.json()["from"], json!(CLOCK_DAY));
    assert_eq!(ahead.json()["to"], json!(CLOCK_DAY));
}
