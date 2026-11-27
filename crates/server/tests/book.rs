//! A portfolio book recorded over HTTP and read back through the holdings view.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    TestApp,
    book_steps::{self, deposit, holding_of, state_of},
    portfolio_steps::portfolio,
};
use rust_decimal::Decimal;
use saffron_core::instrument::InstrumentKind;
use serde_json::{Value, json};

struct Fixture {
    app: TestApp,
    cookie: String,
    portfolio: Value,
    brokerage: i64,
    fund_account: i64,
    stock: i64,
    fund: i64,
    usd: i64,
}

async fn fixture() -> Fixture {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let portfolio = portfolio(&app, &cookie, "Home").await;
    let brokerage = book_steps::account(
        &app,
        &cookie,
        &portfolio,
        json!({ "name": "Brokerage", "kind": "book_entry" }),
    )
    .await;
    let fund_account = book_steps::account(
        &app,
        &cookie,
        &portfolio,
        json!({ "name": "OP funds", "kind": "fund" }),
    )
    .await;
    let stock = book_steps::instrument(&app, "Nokia", InstrumentKind::Stock, "EUR").await;
    let fund = book_steps::instrument(&app, "OP-Suomi", InstrumentKind::Fund, "EUR").await;
    let usd = book_steps::instrument(&app, "Apple", InstrumentKind::Stock, "USD").await;
    Fixture {
        app,
        cookie,
        portfolio,
        brokerage,
        fund_account,
        stock,
        fund,
        usd,
    }
}

fn cents(value: &Value) -> Decimal {
    value
        .as_str()
        .expect("decimal text")
        .parse::<Decimal>()
        .expect("a decimal")
        .round_dp(2)
}

/// Records the fifteen-transaction fixture; returns the first buy and the
/// dividend views.
async fn record_book(f: &Fixture) -> (Value, Value) {
    let brokerage = f.brokerage;
    let funds = f.fund_account;
    let (stock, fund, usd) = (f.stock, f.fund, f.usd);
    let record = |body: Value| book_steps::record(&f.app, &f.cookie, &f.portfolio, body);

    record(deposit(brokerage, "2026-01-02", "10000")).await;
    let buy = record(json!({
        "account_id": brokerage, "instrument_id": stock, "kind": "buy",
        "date": "2026-01-05", "quantity": "10", "price": "100", "fee": "5",
        "note": "first buy",
    }))
    .await;
    assert_eq!(buy["currency"], json!("EUR"), "the account's currency");
    assert_eq!(buy["fee"], json!("5"), "stored in canonical text");
    assert_eq!(buy["note"], json!("first buy"), "the note is echoed");
    record(json!({
        "account_id": brokerage, "instrument_id": stock, "kind": "buy",
        "date": "2026-01-06", "quantity": "10", "price": "110", "fee": "5",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "instrument_id": stock, "kind": "sell",
        "date": "2026-01-07", "quantity": "15", "price": "120", "fee": "6",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "instrument_id": fund, "kind": "buy",
        "date": "2026-01-08", "quantity": "20", "price": "50",
    }))
    .await;
    let dividend = record(json!({
        "account_id": brokerage, "instrument_id": stock, "kind": "dividend",
        "date": "2026-02-01", "amount": "25",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "kind": "fee", "date": "2026-02-02", "amount": "3",
    }))
    .await;
    record(deposit(funds, "2026-02-03", "5000")).await;
    record(json!({
        "account_id": funds, "instrument_id": fund, "kind": "buy",
        "date": "2026-02-04", "quantity": "10", "price": "60", "fee": "2",
    }))
    .await;
    record(json!({
        "account_id": funds, "instrument_id": fund, "kind": "split",
        "date": "2026-02-05", "quantity": "2",
    }))
    .await;
    record(json!({
        "account_id": funds, "instrument_id": fund, "kind": "transfer_out",
        "date": "2026-02-06", "quantity": "5",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "instrument_id": fund, "kind": "transfer_in",
        "date": "2026-02-06", "quantity": "5", "price": "70",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "instrument_id": fund, "kind": "sell",
        "date": "2026-02-07", "quantity": "10", "price": "55",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "instrument_id": usd, "kind": "buy",
        "date": "2026-03-06", "quantity": "8", "price": "25", "fee": "2",
        "currency": "USD", "fx_rate": "0.9",
    }))
    .await;
    record(json!({
        "account_id": brokerage, "instrument_id": usd, "kind": "sell",
        "date": "2026-03-07", "quantity": "4", "price": "27", "fee": "0.5",
        "currency": "USD", "fx_rate": "0.92",
    }))
    .await;
    (buy, dividend)
}

#[tokio::test]
async fn a_recorded_book_reads_back_to_the_cent() {
    let f = fixture().await;
    record_book(&f).await;

    let holdings = book_steps::holdings(&f.app, &f.cookie, &f.portfolio).await;
    let one = state_of(&holdings, f.brokerage);
    assert_eq!(cents(&one["cash"]), "9173.10".parse().expect("decimal"));
    assert_eq!(cents(&one["realized"]), "294.50".parse().expect("decimal"));
    let held_stock = holding_of(one, f.stock);
    assert_eq!(held_stock["quantity"], json!("5"));
    assert_eq!(
        cents(&held_stock["cost_basis"]),
        "552.50".parse().expect("decimal")
    );
    assert_eq!(
        cents(&held_stock["average_cost"]),
        "110.50".parse().expect("decimal")
    );
    let held_fund = holding_of(one, f.fund);
    assert_eq!(held_fund["quantity"], json!("15"));
    assert_eq!(
        cents(&held_fund["cost_basis"]),
        "650.50".parse().expect("decimal")
    );
    assert_eq!(
        cents(&held_fund["average_cost"]),
        "43.37".parse().expect("decimal")
    );
    let lots = held_fund["lots"].as_array().expect("lots");
    assert_eq!(lots.len(), 2);
    assert_eq!(lots[0]["acquired"], json!("2026-01-08"));
    assert_eq!(lots[1]["acquired"], json!("2026-02-04"), "the carried lot");
    assert_eq!(lots[1]["quantity"], json!("5"));
    let held_usd = holding_of(one, f.usd);
    assert_eq!(held_usd["quantity"], json!("4"));
    assert_eq!(
        cents(&held_usd["cost_basis"]),
        "90.90".parse().expect("decimal")
    );
    let two = state_of(&holdings, f.fund_account);
    assert_eq!(cents(&two["cash"]), "4398.00".parse().expect("decimal"));
    assert_eq!(cents(&two["realized"]), "0.00".parse().expect("decimal"));
    let held_in_funds = holding_of(two, f.fund);
    assert_eq!(held_in_funds["quantity"], json!("15"));
    assert_eq!(
        cents(&held_in_funds["cost_basis"]),
        "451.50".parse().expect("decimal")
    );
}

#[tokio::test]
async fn a_book_lists_filters_and_deletes_transactions() {
    let f = fixture().await;
    let (buy, dividend) = record_book(&f).await;

    let listed = f
        .app
        .get(&format!("/api/portfolios/{}/transactions", f.portfolio))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json().as_array().expect("transactions").len(), 15);
    let filtered = f
        .app
        .get(&format!(
            "/api/portfolios/{}/transactions?account_id={}",
            f.portfolio, f.fund_account
        ))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(
        filtered.json().as_array().expect("transactions").len(),
        4,
        "the fund account's own rows"
    );

    let removed = f
        .app
        .call(
            Method::DELETE,
            &format!(
                "/api/portfolios/{}/transactions/{}",
                f.portfolio,
                dividend["id"].as_i64().expect("dividend id")
            ),
        )
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(
        removed.status,
        StatusCode::NO_CONTENT,
        "{:?}",
        removed.json()
    );
    let after = book_steps::holdings(&f.app, &f.cookie, &f.portfolio).await;
    assert_eq!(
        cents(&state_of(&after, f.brokerage)["cash"]),
        "9148.10".parse().expect("decimal"),
        "the dividend is gone"
    );

    let refused = f
        .app
        .call(
            Method::DELETE,
            &format!(
                "/api/portfolios/{}/transactions/{}",
                f.portfolio,
                buy["id"].as_i64().expect("buy id")
            ),
        )
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["error"], json!("would_break_book"));
}

#[tokio::test]
async fn an_account_can_be_renamed_closed_and_reopened() {
    let f = fixture().await;
    let base = format!("/api/portfolios/{}/accounts/{}", f.portfolio, f.brokerage);

    let renamed = f
        .app
        .call(Method::PATCH, &base)
        .cookie(&f.cookie)
        .json(json!({ "name": "Nordnet" }))
        .send()
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.json());
    assert_eq!(renamed.json()["name"], json!("Nordnet"));

    let closed = f
        .app
        .post(&format!("{base}/close"))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(closed.status, StatusCode::OK);
    assert!(!closed.json()["closed_at"].is_null());
    let refused = f
        .app
        .post(&format!("/api/portfolios/{}/transactions", f.portfolio))
        .cookie(&f.cookie)
        .json(deposit(f.brokerage, "2026-01-05", "10"))
        .send()
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["error"], json!("account_closed"));

    let reopened = f
        .app
        .post(&format!("{base}/reopen"))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(reopened.status, StatusCode::OK);
    assert!(reopened.json()["closed_at"].is_null());
    let accepted = f
        .app
        .post(&format!("/api/portfolios/{}/transactions", f.portfolio))
        .cookie(&f.cookie)
        .json(deposit(f.brokerage, "2026-01-05", "10"))
        .send()
        .await;
    assert_eq!(accepted.status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_transaction_can_be_edited_in_place() {
    let f = fixture().await;
    let record = |body: Value| book_steps::record(&f.app, &f.cookie, &f.portfolio, body);
    record(deposit(f.brokerage, "2026-01-02", "10000")).await;
    let buy = record(json!({
        "account_id": f.brokerage, "instrument_id": f.stock, "kind": "buy",
        "date": "2026-01-05", "quantity": "10", "price": "100", "fee": "5",
        "note": "typo",
    }))
    .await;
    let id = buy["id"].as_i64().expect("buy id");

    let edited = f
        .app
        .call(
            Method::PATCH,
            &format!("/api/portfolios/{}/transactions/{id}", f.portfolio),
        )
        .cookie(&f.cookie)
        .json(json!({
            "account_id": f.brokerage, "instrument_id": f.stock, "kind": "buy",
            "date": "2026-01-05", "quantity": "4", "price": "100", "fee": "5",
            "note": "fixed",
        }))
        .send()
        .await;

    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.json());
    assert_eq!(edited.json()["id"], json!(id), "the row keeps its id");
    assert_eq!(edited.json()["quantity"], json!("4"));
    assert_eq!(edited.json()["note"], json!("fixed"));
    let holdings = book_steps::holdings(&f.app, &f.cookie, &f.portfolio).await;
    let held = holding_of(state_of(&holdings, f.brokerage), f.stock);
    assert_eq!(held["quantity"], json!("4"));
    assert_eq!(
        cents(&held["cost_basis"]),
        "405.00".parse().expect("decimal")
    );
    let listed = f
        .app
        .get(&format!("/api/portfolios/{}/transactions", f.portfolio))
        .cookie(&f.cookie)
        .send()
        .await;
    let rows = listed.json();
    let rows = rows.as_array().expect("transactions");
    assert_eq!(rows.len(), 2, "no row was added or removed");
    assert_eq!(
        rows.iter()
            .find(|row| row["id"].as_i64() == Some(id))
            .expect("the edited row")["note"],
        json!("fixed")
    );
}

#[tokio::test]
async fn an_edit_that_breaks_the_book_is_refused() {
    let f = fixture().await;
    let record = |body: Value| book_steps::record(&f.app, &f.cookie, &f.portfolio, body);
    record(deposit(f.brokerage, "2026-01-02", "10000")).await;
    let buy = record(json!({
        "account_id": f.brokerage, "instrument_id": f.stock, "kind": "buy",
        "date": "2026-01-05", "quantity": "5", "price": "100",
    }))
    .await;
    let id = buy["id"].as_i64().expect("buy id");
    record(json!({
        "account_id": f.brokerage, "instrument_id": f.stock, "kind": "sell",
        "date": "2026-01-06", "quantity": "5", "price": "110",
    }))
    .await;
    let edit = |quantity: &str, date: &str| {
        json!({
            "account_id": f.brokerage, "instrument_id": f.stock, "kind": "buy",
            "date": date, "quantity": quantity, "price": "100",
        })
    };

    for body in [edit("4", "2026-01-05"), edit("5", "2026-01-07")] {
        let refused = f
            .app
            .call(
                Method::PATCH,
                &format!("/api/portfolios/{}/transactions/{id}", f.portfolio),
            )
            .cookie(&f.cookie)
            .json(body)
            .send()
            .await;
        assert_eq!(refused.status, StatusCode::CONFLICT, "{:?}", refused.json());
        assert_eq!(refused.json()["error"], json!("would_break_book"));
    }

    let listed = f
        .app
        .get(&format!("/api/portfolios/{}/transactions", f.portfolio))
        .cookie(&f.cookie)
        .send()
        .await;
    let rows = listed.json();
    let kept = rows
        .as_array()
        .expect("transactions")
        .iter()
        .find(|row| row["id"].as_i64() == Some(id))
        .expect("the edited row");
    assert_eq!(
        kept["quantity"],
        json!("5"),
        "a refused edit leaves the row as it was"
    );

    let missing = f
        .app
        .call(
            Method::PATCH,
            &format!("/api/portfolios/{}/transactions/999999", f.portfolio),
        )
        .cookie(&f.cookie)
        .json(edit("5", "2026-01-05"))
        .send()
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_closed_account_refuses_an_edit_or_a_deletion() {
    let f = fixture().await;
    let deposit_row = book_steps::record(
        &f.app,
        &f.cookie,
        &f.portfolio,
        deposit(f.brokerage, "2026-01-02", "10"),
    )
    .await;
    let id = deposit_row["id"].as_i64().expect("deposit id");
    f.app
        .post(&format!(
            "/api/portfolios/{}/accounts/{}/close",
            f.portfolio, f.brokerage
        ))
        .cookie(&f.cookie)
        .send()
        .await;

    let refused = f
        .app
        .call(
            Method::PATCH,
            &format!("/api/portfolios/{}/transactions/{id}", f.portfolio),
        )
        .cookie(&f.cookie)
        .json(deposit(f.brokerage, "2026-01-02", "20"))
        .send()
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["error"], json!("account_closed"));

    // Moving the row into an open account would still take it out of the closed one.
    let moved = f
        .app
        .call(
            Method::PATCH,
            &format!("/api/portfolios/{}/transactions/{id}", f.portfolio),
        )
        .cookie(&f.cookie)
        .json(deposit(f.fund_account, "2026-01-02", "20"))
        .send()
        .await;
    assert_eq!(moved.status, StatusCode::CONFLICT);
    assert_eq!(moved.json()["error"], json!("account_closed"));

    let removed = f
        .app
        .call(
            Method::DELETE,
            &format!("/api/portfolios/{}/transactions/{id}", f.portfolio),
        )
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(removed.status, StatusCode::CONFLICT);
    assert_eq!(removed.json()["error"], json!("account_closed"));
}
