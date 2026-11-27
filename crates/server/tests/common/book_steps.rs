//! Steps for the book tests: instruments, accounts, recorded transactions, and the
//! holdings view.

use axum::http::StatusCode;
use jiff::Timestamp;
use saffron_core::instrument::InstrumentKind;
use saffron_db::instruments::{self, NewInstrument, NewListing};
use serde_json::{Value, json};

use super::TestApp;

/// Stores an instrument directly, without a price provider.
pub async fn instrument(app: &TestApp, name: &str, kind: InstrumentKind, currency: &str) -> i64 {
    let new = NewInstrument {
        kind,
        name,
        isin: None,
        currency,
    };
    let listing = NewListing {
        provider: "test",
        symbol: name,
        exchange: None,
        currency,
        timezone: None,
    };
    let now = "2026-01-01T00:00:00Z"
        .parse::<Timestamp>()
        .expect("timestamp");
    instruments::create(&app.state.pool, new, listing, now)
        .await
        .expect("instrument")
}

/// Creates an account through the API; returns its id.
pub async fn account(app: &TestApp, cookie: &str, portfolio: &Value, body: Value) -> i64 {
    let reply = app
        .post(&format!(
            "/api/portfolios/{}/accounts",
            portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(cookie)
        .json(body)
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    reply.json()["id"].as_i64().expect("account id")
}

/// Records a transaction through the API; returns its view.
pub async fn record(app: &TestApp, cookie: &str, portfolio: &Value, body: Value) -> Value {
    let reply = app
        .post(&format!(
            "/api/portfolios/{}/transactions",
            portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(cookie)
        .json(body)
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    reply.json()
}

/// The holdings view of the portfolio.
pub async fn holdings(app: &TestApp, cookie: &str, portfolio: &Value) -> Vec<Value> {
    let reply = app
        .get(&format!(
            "/api/portfolios/{}/holdings",
            portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(cookie)
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
    reply
        .json()
        .as_array()
        .expect("the holdings are a list")
        .clone()
}

/// The state of one account in a holdings view.
pub fn state_of(holdings: &[Value], account: i64) -> &Value {
    holdings
        .iter()
        .find(|state| state["account_id"].as_i64() == Some(account))
        .expect("the account's state")
}

/// The holding of one instrument in an account's state.
pub fn holding_of(state: &Value, instrument: i64) -> &Value {
    state["holdings"]
        .as_array()
        .expect("holdings are a list")
        .iter()
        .find(|holding| holding["instrument_id"].as_i64() == Some(instrument))
        .expect("the holding")
}

/// A deposit body.
#[must_use]
pub fn deposit(account: i64, date: &str, amount: &str) -> Value {
    json!({ "account_id": account, "kind": "deposit", "date": date, "amount": amount })
}
