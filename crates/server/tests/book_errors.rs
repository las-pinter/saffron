//! What the book routes refuse: invalid transactions, roles, and portfolios the
//! caller may not touch.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    TestApp,
    book_steps::{self, deposit, state_of},
    portfolio_steps::{self, Household, household},
};
use saffron_core::instrument::InstrumentKind;
use serde_json::{Value, json};

struct Fixture {
    household: Household,
    portfolio: Value,
    account: i64,
    stock: i64,
}

async fn fixture() -> Fixture {
    let household = household().await;
    let portfolio = portfolio_steps::portfolio(&household.app, &household.admin, "Home").await;
    let account = book_steps::account(
        &household.app,
        &household.admin,
        &portfolio,
        json!({ "name": "Brokerage", "kind": "book_entry" }),
    )
    .await;
    let stock = book_steps::instrument(&household.app, "Nokia", InstrumentKind::Stock, "EUR").await;
    Fixture {
        household,
        portfolio,
        account,
        stock,
    }
}

impl Fixture {
    fn app(&self) -> &TestApp {
        &self.household.app
    }

    fn path(&self, suffix: &str) -> String {
        format!("/api/portfolios/{}/{}", self.portfolio, suffix)
    }

    async fn post_transaction(&self, cookie: &str, body: Value) -> common::Reply {
        self.app()
            .post(&self.path("transactions"))
            .cookie(cookie)
            .json(body)
            .send()
            .await
    }
}

#[tokio::test]
async fn an_oversale_is_refused() {
    let f = fixture().await;
    let admin = &f.household.admin;
    book_steps::record(
        f.app(),
        admin,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "buy",
            "date": "2026-01-05", "quantity": "5", "price": "10",
        }),
    )
    .await;

    let reply = f
        .post_transaction(
            admin,
            json!({
                "account_id": f.account, "instrument_id": f.stock, "kind": "sell",
                "date": "2026-01-06", "quantity": "6", "price": "11",
            }),
        )
        .await;

    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.json()["error"], json!("oversold"));
}

#[tokio::test]
async fn a_malformed_transaction_is_refused() {
    let f = fixture().await;
    let admin = &f.household.admin;
    let cases = [
        (
            json!({
                "account_id": f.account, "kind": "barter", "date": "2026-01-05",
                "amount": "5",
            }),
            "invalid_kind",
        ),
        (
            json!({
                "account_id": f.account, "kind": "deposit", "date": "2026-01-32",
                "amount": "5",
            }),
            "invalid_date",
        ),
        (
            json!({
                "account_id": f.account, "kind": "deposit", "date": "2026-01-05",
                "amount": "five",
            }),
            "invalid_decimal",
        ),
        (
            json!({ "account_id": f.account, "kind": "deposit", "date": "2026-01-05" }),
            "invalid_transaction",
        ),
        (
            json!({
                "account_id": f.account, "instrument_id": f.stock, "kind": "buy",
                "date": "2026-01-05", "quantity": "5", "price": "10",
                "currency": "USD",
            }),
            "invalid_transaction",
        ),
        (
            json!({
                "account_id": f.account, "kind": "deposit", "date": "2026-01-05",
                "amount": "5", "fee": "-1",
            }),
            "invalid_transaction",
        ),
    ];
    for (body, expected) in cases {
        let reply = f.post_transaction(admin, body.clone()).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(reply.json()["error"], json!(expected), "{body}");
    }
}

#[tokio::test]
async fn an_unknown_instrument_or_account_is_refused() {
    let f = fixture().await;
    let admin = &f.household.admin;
    let unknown_instrument = f
        .post_transaction(
            admin,
            json!({
                "account_id": f.account, "instrument_id": 999_999, "kind": "buy",
                "date": "2026-01-05", "quantity": "5", "price": "10",
            }),
        )
        .await;
    assert_eq!(unknown_instrument.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        unknown_instrument.json()["error"],
        json!("unknown_instrument")
    );

    let unknown_account = f
        .post_transaction(admin, deposit(999_999, "2026-01-05", "5"))
        .await;
    assert_eq!(unknown_account.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_transfer_must_stay_in_one_currency() {
    let f = fixture().await;
    let admin = &f.household.admin;
    let dollars = book_steps::account(
        f.app(),
        admin,
        &f.portfolio,
        json!({ "name": "US brokerage", "kind": "book_entry", "currency": "USD" }),
    )
    .await;
    book_steps::record(
        f.app(),
        admin,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "buy",
            "date": "2026-01-05", "quantity": "5", "price": "10",
        }),
    )
    .await;
    book_steps::record(
        f.app(),
        admin,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "transfer_out",
            "date": "2026-01-06", "quantity": "5",
        }),
    )
    .await;

    let reply = f
        .post_transaction(
            admin,
            json!({
                "account_id": dollars, "instrument_id": f.stock, "kind": "transfer_in",
                "date": "2026-01-06", "quantity": "5", "price": "11",
            }),
        )
        .await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.json()["error"], json!("transfer_currency"));
}

#[tokio::test]
async fn a_viewer_reads_but_does_not_write() {
    let f = fixture().await;
    portfolio_steps::share(
        f.app(),
        &f.household.admin,
        &f.portfolio,
        "member",
        "viewer",
    )
    .await;
    let member = &f.household.member;

    let holdings = f.app().get(&f.path("holdings")).cookie(member).send().await;
    assert_eq!(holdings.status, StatusCode::OK);
    let accounts = f.app().get(&f.path("accounts")).cookie(member).send().await;
    assert_eq!(accounts.status, StatusCode::OK);

    let writes = [
        (
            Method::POST,
            f.path("accounts"),
            Some(json!({ "name": "New", "kind": "cash" })),
        ),
        (
            Method::POST,
            f.path("transactions"),
            Some(deposit(f.account, "2026-01-05", "5")),
        ),
        (Method::DELETE, f.path("transactions/1"), None),
        (
            Method::PATCH,
            f.path(&format!("accounts/{}", f.account)),
            Some(json!({ "name": "Renamed" })),
        ),
        (
            Method::POST,
            f.path(&format!("accounts/{}/close", f.account)),
            None,
        ),
        (
            Method::POST,
            f.path(&format!("accounts/{}/reopen", f.account)),
            None,
        ),
    ];
    for (method, path, body) in writes {
        let call = f.app().call(method, &path).cookie(member);
        let reply = match body {
            Some(body) => call.json(body).send().await,
            None => call.send().await,
        };
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn an_editor_may_write() {
    let f = fixture().await;
    portfolio_steps::share(
        f.app(),
        &f.household.admin,
        &f.portfolio,
        "member",
        "editor",
    )
    .await;
    let member = &f.household.member;

    let account = f
        .app()
        .post(&f.path("accounts"))
        .cookie(member)
        .json(json!({ "name": "Member cash", "kind": "cash", "currency": "EUR" }))
        .send()
        .await;
    assert_eq!(account.status, StatusCode::CREATED);
    let recorded = f
        .post_transaction(member, deposit(f.account, "2026-01-05", "5"))
        .await;
    assert_eq!(recorded.status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_backdated_write_that_breaks_a_stored_row_is_a_conflict() {
    let f = fixture().await;
    let admin = &f.household.admin;
    book_steps::record(
        f.app(),
        admin,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "buy",
            "date": "2026-01-05", "quantity": "10", "price": "10",
        }),
    )
    .await;
    book_steps::record(
        f.app(),
        admin,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "sell",
            "date": "2026-01-10", "quantity": "5", "price": "11",
        }),
    )
    .await;

    let reply = f
        .post_transaction(
            admin,
            json!({
                "account_id": f.account, "instrument_id": f.stock, "kind": "transfer_out",
                "date": "2026-01-07", "quantity": "8",
            }),
        )
        .await;

    assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.json());
    assert_eq!(reply.json()["error"], json!("would_break_book"));
}

#[tokio::test]
async fn a_strangers_portfolio_is_not_found() {
    let f = fixture().await;
    let member = &f.household.member;

    for path in [
        f.path("accounts"),
        f.path("transactions"),
        f.path("holdings"),
    ] {
        let reply = f.app().get(&path).cookie(member).send().await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
    }
    let writes = [
        (Method::POST, f.path("accounts")),
        (Method::PATCH, f.path(&format!("accounts/{}", f.account))),
        (
            Method::POST,
            f.path(&format!("accounts/{}/close", f.account)),
        ),
        (
            Method::POST,
            f.path(&format!("accounts/{}/reopen", f.account)),
        ),
        (Method::POST, f.path("transactions")),
        (Method::DELETE, f.path("transactions/1")),
    ];
    for (method, path) in writes {
        let reply = f
            .app()
            .call(method, &path)
            .cookie(member)
            .json(deposit(f.account, "2026-01-05", "5"))
            .send()
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn an_archived_portfolio_refuses_writes() {
    let f = fixture().await;
    let admin = &f.household.admin;
    let archived = f.app().post(&f.path("archive")).cookie(admin).send().await;
    assert_eq!(archived.status, StatusCode::OK);

    let holdings = f.app().get(&f.path("holdings")).cookie(admin).send().await;
    assert_eq!(holdings.status, StatusCode::OK);
    let write = f
        .post_transaction(admin, deposit(f.account, "2026-01-05", "5"))
        .await;
    assert_eq!(write.status, StatusCode::CONFLICT);
    assert_eq!(write.json()["error"], json!("archived"));
    let new_account = f
        .app()
        .post(&f.path("accounts"))
        .cookie(admin)
        .json(json!({ "name": "New", "kind": "cash" }))
        .send()
        .await;
    assert_eq!(new_account.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn two_overlapping_sells_cannot_oversell_the_book() {
    let f = fixture().await;
    let admin = f.household.admin.clone();
    book_steps::record(
        f.app(),
        &admin,
        &f.portfolio,
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "buy",
            "date": "2026-01-05", "quantity": "5", "price": "10",
        }),
    )
    .await;
    let sale = || {
        json!({
            "account_id": f.account, "instrument_id": f.stock, "kind": "sell",
            "date": "2026-01-06", "quantity": "5", "price": "11",
        })
    };
    let path = f.path("transactions");

    let (first, second) = tokio::join!(
        f.app().post(&path).cookie(&admin).json(sale()).send(),
        f.app().post(&path).cookie(&admin).json(sale()).send(),
    );

    let statuses = [first.status, second.status];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CREATED)
            .count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1,
        "{statuses:?}"
    );
    let holdings = book_steps::holdings(f.app(), &admin, &f.portfolio).await;
    assert!(
        state_of(&holdings, f.account)["holdings"]
            .as_array()
            .expect("holdings")
            .is_empty(),
        "the single position is sold at most once"
    );
}

#[tokio::test]
async fn deleting_a_transaction_that_does_not_exist_is_not_found() {
    let f = fixture().await;

    let reply = f
        .app()
        .call(Method::DELETE, &f.path("transactions/999999"))
        .cookie(&f.household.admin)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}
