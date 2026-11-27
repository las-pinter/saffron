//! CSV import and export over HTTP: a preview with duplicates, an all-or-nothing
//! import, and the two export forms.

mod common;

use axum::http::StatusCode;
use common::{
    Reply, TestApp, book_steps,
    portfolio_steps::{household, portfolio, share},
};
use jiff::Timestamp;
use saffron_core::instrument::InstrumentKind;
use saffron_db::instruments::{self, NewInstrument, NewListing};
use serde_json::{Value, json};

struct Fixture {
    app: TestApp,
    cookie: String,
    portfolio: Value,
    account: i64,
    stock: i64,
}

async fn fixture() -> Fixture {
    let app = TestApp::new().await;
    let cookie = app.setup_admin().await;
    let portfolio = portfolio(&app, &cookie, "Home").await;
    let account = book_steps::account(
        &app,
        &cookie,
        &portfolio,
        json!({ "name": "Brokerage", "kind": "book_entry" }),
    )
    .await;
    let stock = book_steps::instrument(&app, "Nokia", InstrumentKind::Stock, "EUR").await;
    Fixture {
        app,
        cookie,
        portfolio,
        account,
        stock,
    }
}

fn columns() -> Value {
    json!({
        "account": "account", "kind": "kind", "date": "date",
        "instrument": "instrument", "quantity": "quantity", "price": "price",
        "amount": "amount", "fee": "fee", "currency": "currency",
        "fx_rate": "fx_rate", "note": "note",
    })
}

fn csv_text(records: &[Vec<String>]) -> String {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for record in records {
        writer.write_record(record).expect("a record");
    }
    String::from_utf8(writer.into_inner().expect("CSV")).expect("UTF-8")
}

fn header() -> Vec<String> {
    [
        "account",
        "kind",
        "date",
        "instrument",
        "quantity",
        "price",
        "amount",
        "fee",
        "currency",
        "fx_rate",
        "note",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn row(values: [&str; 11]) -> Vec<String> {
    values.into_iter().map(str::to_owned).collect()
}

async fn post_csv(f: &Fixture, path: &str, csv: &str, columns: Value) -> Reply {
    f.app
        .post(&format!(
            "/api/portfolios/{}{path}",
            f.portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .json(json!({ "csv": csv, "columns": columns }))
        .send()
        .await
}

async fn import(f: &Fixture, csv: &str) -> Reply {
    post_csv(f, "/import", csv, columns()).await
}

async fn preview(f: &Fixture, csv: &str) -> Reply {
    post_csv(f, "/import/preview", csv, columns()).await
}

async fn transactions(f: &Fixture) -> Vec<Value> {
    f.app
        .get(&format!(
            "/api/portfolios/{}/transactions",
            f.portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .send()
        .await
        .json()
        .as_array()
        .expect("transactions")
        .clone()
}

async fn holdings(f: &Fixture) -> Vec<Value> {
    f.app
        .get(&format!(
            "/api/portfolios/{}/holdings",
            f.portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .send()
        .await
        .json()
        .as_array()
        .expect("holdings")
        .clone()
}

fn hundred_rows() -> String {
    let mut records = vec![header()];
    records.push(row([
        "Brokerage",
        "deposit",
        "2026-01-02",
        "",
        "",
        "",
        "10000",
        "",
        "EUR",
        "",
        "opening",
    ]));
    for quantity in 1..=99 {
        records.push(row([
            "Brokerage",
            "buy",
            "2026-01-03",
            "Nokia",
            &quantity.to_string(),
            "1",
            "",
            "",
            "EUR",
            "",
            "",
        ]));
    }
    csv_text(&records)
}

#[tokio::test]
async fn a_hundred_row_file_imports_exactly() {
    let f = fixture().await;
    let csv = hundred_rows();

    let preview = preview(&f, &csv).await;
    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.json());
    assert_eq!(
        (
            preview.json()["rows"].clone(),
            preview.json()["importable"].clone(),
            preview.json()["duplicates"].clone(),
            preview.json()["invalid"].clone(),
        ),
        (json!(100), json!(100), json!(0), json!(0))
    );
    assert_eq!(
        preview.json()["detail"][0]["transaction"]["kind"],
        json!("deposit")
    );

    let imported = import(&f, &csv).await;
    assert_eq!(imported.status, StatusCode::OK, "{:?}", imported.json());
    assert_eq!(imported.json(), json!({ "imported": 100, "duplicates": 0 }));

    let stored = transactions(&f).await;
    assert_eq!(stored.len(), 100);
    let state = holdings(&f)
        .await
        .into_iter()
        .find(|state| state["account_id"] == json!(f.account))
        .expect("the account");
    assert_eq!(state["cash"], json!("5050"), "10000 minus 4950");
    assert_eq!(state["holdings"][0]["quantity"], json!("4950"));
}

#[tokio::test]
async fn re_importing_creates_no_duplicates() {
    let f = fixture().await;
    let csv = hundred_rows();
    assert_eq!(import(&f, &csv).await.status, StatusCode::OK);

    let again = import(&f, &csv).await;

    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json(), json!({ "imported": 0, "duplicates": 100 }));
    assert_eq!(transactions(&f).await.len(), 100);
    let preview = preview(&f, &csv).await;
    assert_eq!(preview.json()["duplicates"], json!(100));
    assert_eq!(preview.json()["importable"], json!(0));
}

#[tokio::test]
async fn a_row_repeated_in_the_file_is_imported_once() {
    let f = fixture().await;
    let deposit = row([
        "Brokerage",
        "deposit",
        "2026-01-02",
        "",
        "",
        "",
        "100",
        "",
        "EUR",
        "",
        "",
    ]);
    let csv = csv_text(&[header(), deposit.clone(), deposit]);

    let preview = preview(&f, &csv).await;
    assert_eq!(preview.json()["importable"], json!(1));
    assert_eq!(preview.json()["duplicates"], json!(1));

    let imported = import(&f, &csv).await;
    assert_eq!(imported.json(), json!({ "imported": 1, "duplicates": 1 }));
    assert_eq!(transactions(&f).await.len(), 1);
}

#[tokio::test]
async fn a_bad_row_refuses_the_whole_import() {
    let f = fixture().await;
    let csv = csv_text(&[
        header(),
        row([
            "Brokerage",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Brokerage",
            "sell",
            "2026-01-03",
            "Nokia",
            "5",
            "10",
            "",
            "",
            "EUR",
            "",
            "",
        ]),
    ]);

    let refused = import(&f, &csv).await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("import_invalid"));
    assert!(
        transactions(&f).await.is_empty(),
        "nothing is written when one row is refused"
    );
}

#[tokio::test]
async fn a_preview_names_each_bad_row_without_writing() {
    let f = fixture().await;
    let csv = csv_text(&[
        header(),
        row([
            "Ghost",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Brokerage",
            "teleport",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Brokerage",
            "deposit",
            "yesterday",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
    ]);

    let preview = preview(&f, &csv).await;

    assert_eq!(preview.status, StatusCode::OK);
    assert_eq!(preview.json()["invalid"], json!(3));
    assert_eq!(preview.json()["importable"], json!(0));
    let detail = preview.json()["detail"].as_array().expect("detail").clone();
    assert_eq!(
        detail[0]["errors"][0],
        json!("account `Ghost` is not in this portfolio")
    );
    assert_eq!(
        detail[1]["errors"][0],
        json!("`teleport` is not a transaction kind")
    );
    assert_eq!(
        detail[2]["errors"][0],
        json!("date `yesterday` is not YYYY-MM-DD")
    );
    assert_eq!(transactions(&f).await, Vec::<Value>::new());
}

/// The portfolio's transactions as the CSV export writes them.
async fn exported_csv(f: &Fixture) -> String {
    let exported = f
        .app
        .get(&format!(
            "/api/portfolios/{}/export.csv",
            f.portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(exported.status, StatusCode::OK);
    assert_eq!(
        exported.header("content-type"),
        Some("text/csv; charset=utf-8")
    );
    String::from_utf8(exported.body.clone()).expect("CSV text")
}

/// Account ids differ between portfolios; everything else must match.
fn strip_ids(states: &[Value]) -> Vec<Value> {
    states
        .iter()
        .map(|state| {
            let mut state = state.clone();
            state["account_id"] = Value::Null;
            state
        })
        .collect()
}

#[tokio::test]
async fn a_mapping_must_name_real_columns() {
    let f = fixture().await;
    let csv = hundred_rows();

    let mut wrong_name = columns();
    wrong_name["account"] = json!("owner");
    let unknown = post_csv(&f, "/import/preview", &csv, wrong_name).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown.json()["error"], json!("unknown_column"));

    let mut missing = columns();
    missing.as_object_mut().expect("object").remove("kind");
    let refused = post_csv(&f, "/import/preview", &csv, missing).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("missing_column"));

    let empty = post_csv(&f, "/import/preview", "", columns()).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn export_then_import_reproduces_the_holdings() {
    let f = fixture().await;
    let record = |body: Value| book_steps::record(&f.app, &f.cookie, &f.portfolio, body);
    record(book_steps::deposit(f.account, "2026-01-02", "1000")).await;
    record(json!({
        "account_id": f.account, "instrument_id": f.stock, "kind": "buy",
        "date": "2026-01-05", "quantity": "10", "price": "50", "fee": "2",
    }))
    .await;
    record(json!({
        "account_id": f.account, "instrument_id": f.stock, "kind": "sell",
        "date": "2026-01-10", "quantity": "4", "price": "60", "fee": "1",
    }))
    .await;
    record(json!({
        "account_id": f.account, "instrument_id": f.stock, "kind": "dividend",
        "date": "2026-02-01", "amount": "5",
    }))
    .await;
    let before = holdings(&f).await;
    let csv = exported_csv(&f).await;
    assert!(
        csv.starts_with(
            "account,kind,date,instrument,quantity,price,amount,fee,currency,fx_rate,note"
        ),
        "{csv}"
    );
    assert_eq!(csv.lines().count(), 5, "a header and four transactions");

    let other = portfolio(&f.app, &f.cookie, "Imported").await;
    book_steps::account(
        &f.app,
        &f.cookie,
        &other,
        json!({ "name": "Brokerage", "kind": "book_entry" }),
    )
    .await;
    let reply = f
        .app
        .post(&format!(
            "/api/portfolios/{}/import",
            other.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .json(json!({ "csv": csv, "columns": columns() }))
        .send()
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json());
    assert_eq!(reply.json()["imported"], json!(4));

    let imported = f
        .app
        .get(&format!(
            "/api/portfolios/{}/holdings",
            other.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .send()
        .await
        .json()
        .as_array()
        .expect("holdings")
        .clone();
    assert_eq!(strip_ids(&imported), strip_ids(&before));

    let exported = f
        .app
        .get(&format!(
            "/api/portfolios/{}/export.json",
            f.portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(exported.status, StatusCode::OK);
    let body = exported.json();
    assert_eq!(body["portfolio"]["name"], json!("Home"));
    assert_eq!(body["accounts"].as_array().expect("accounts").len(), 1);
    assert_eq!(
        body["transactions"].as_array().expect("transactions").len(),
        4
    );
    assert_eq!(body["holdings"], json!(before));
}

#[tokio::test]
async fn a_note_does_not_change_duplicate_detection() {
    let f = fixture().await;
    let deposit = |note: &str| {
        csv_text(&[
            header(),
            row([
                "Brokerage",
                "deposit",
                "2026-01-02",
                "",
                "",
                "",
                "100",
                "",
                "EUR",
                "",
                note,
            ]),
        ])
    };
    assert_eq!(
        import(&f, &deposit("first note")).await.json()["imported"],
        json!(1)
    );

    let again = import(&f, &deposit("rewritten note")).await;

    assert_eq!(again.json(), json!({ "imported": 0, "duplicates": 1 }));
}

#[tokio::test]
async fn a_duplicate_before_a_bad_row_names_the_bad_line() {
    let f = fixture().await;
    let deposit = row([
        "Brokerage",
        "deposit",
        "2026-01-02",
        "",
        "",
        "",
        "100",
        "",
        "EUR",
        "",
        "",
    ]);
    assert_eq!(
        import(&f, &csv_text(&[header(), deposit.clone()]))
            .await
            .json()["imported"],
        json!(1)
    );
    let sell = row([
        "Brokerage",
        "sell",
        "2026-01-03",
        "Nokia",
        "5",
        "10",
        "",
        "",
        "EUR",
        "",
        "",
    ]);

    let refused = import(&f, &csv_text(&[header(), deposit, sell])).await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("import_invalid"));
    assert!(
        refused.json()["message"]
            .as_str()
            .expect("message")
            .starts_with("data row 2:"),
        "{:?}",
        refused.json()
    );
}

#[tokio::test]
async fn a_cross_currency_transfer_names_its_row() {
    let f = fixture().await;
    let _ = book_steps::account(
        &f.app,
        &f.cookie,
        &f.portfolio,
        json!({ "name": "Dollars", "kind": "book_entry", "currency": "USD" }),
    )
    .await;
    let csv = csv_text(&[
        header(),
        row([
            "Brokerage",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Brokerage",
            "buy",
            "2026-01-02",
            "Nokia",
            "1",
            "10",
            "",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Brokerage",
            "transfer_out",
            "2026-01-03",
            "Nokia",
            "1",
            "",
            "",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Dollars",
            "transfer_in",
            "2026-01-03",
            "Nokia",
            "1",
            "",
            "",
            "",
            "USD",
            "",
            "",
        ]),
    ]);
    let refused = import(&f, &csv).await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("import_invalid"));
    let message = refused.json()["message"]
        .as_str()
        .expect("message")
        .to_owned();
    assert!(message.starts_with("data row "), "{message}");
    assert_eq!(transactions(&f).await.len(), 0, "nothing is written");
}

#[tokio::test]
async fn a_file_beyond_the_row_cap_is_refused() {
    let f = fixture().await;
    let mut records = vec![header()];
    for _ in 0..=saffron_server::imports::MAX_ROWS {
        records.push(row([
            "Brokerage",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "1",
            "",
            "EUR",
            "",
            "",
        ]));
    }

    let refused = preview(&f, &csv_text(&records)).await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("too_many_rows"));
}

#[tokio::test]
async fn an_archived_portfolio_can_be_previewed_not_imported() {
    let f = fixture().await;
    let csv = csv_text(&[
        header(),
        row([
            "Brokerage",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
    ]);
    let archived = f
        .app
        .post(&format!(
            "/api/portfolios/{}/archive",
            f.portfolio.as_i64().expect("portfolio id")
        ))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(archived.status, StatusCode::OK, "{:?}", archived.json());

    let previewed = preview(&f, &csv).await;
    assert_eq!(previewed.status, StatusCode::OK, "{:?}", previewed.json());
    assert_eq!(previewed.json()["importable"], json!(1));
    let refused = import(&f, &csv).await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["error"], json!("archived"));
}

#[tokio::test]
async fn a_closed_account_still_recognizes_its_duplicates() {
    let f = fixture().await;
    let deposit = |date: &str, amount: &str| {
        csv_text(&[
            header(),
            row([
                "Brokerage",
                "deposit",
                date,
                "",
                "",
                "",
                amount,
                "",
                "EUR",
                "",
                "",
            ]),
        ])
    };
    let original = deposit("2026-01-02", "100");
    assert_eq!(import(&f, &original).await.json()["imported"], json!(1));
    let closed = f
        .app
        .post(&format!(
            "/api/portfolios/{}/accounts/{}/close",
            f.portfolio.as_i64().expect("portfolio id"),
            f.account
        ))
        .cookie(&f.cookie)
        .send()
        .await;
    assert_eq!(closed.status, StatusCode::OK, "{:?}", closed.json());

    let again = import(&f, &original).await;
    assert_eq!(again.status, StatusCode::OK, "{:?}", again.json());
    assert_eq!(again.json(), json!({ "imported": 0, "duplicates": 1 }));

    let new = deposit("2026-01-03", "50");
    let previewed = preview(&f, &new).await;
    assert_eq!(previewed.json()["invalid"], json!(1));
    assert_eq!(
        previewed.json()["detail"][0]["errors"][0],
        json!("account `Brokerage` is closed")
    );
    let refused = import(&f, &new).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("import_invalid"));
}

#[tokio::test]
async fn an_instrument_isin_resolves() {
    let f = fixture().await;
    let now = "2026-01-01T00:00:00Z"
        .parse::<Timestamp>()
        .expect("timestamp");
    let fund = instruments::create(
        &f.app.state.pool,
        NewInstrument {
            kind: InstrumentKind::Fund,
            name: "OP-Suomi Indeksi B",
            isin: Some("FI4000315700"),
            currency: "EUR",
        },
        NewListing {
            provider: "test",
            symbol: "0P0001DHRJ.F",
            exchange: None,
            currency: "EUR",
            timezone: None,
        },
        now,
    )
    .await
    .expect("instrument");
    let csv = csv_text(&[
        header(),
        row([
            "Brokerage",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
        row([
            "Brokerage",
            "buy",
            "2026-01-02",
            "FI4000315700",
            "2",
            "10",
            "",
            "",
            "EUR",
            "",
            "",
        ]),
    ]);

    let imported = import(&f, &csv).await;

    assert_eq!(imported.status, StatusCode::OK, "{:?}", imported.json());
    let state = holdings(&f)
        .await
        .into_iter()
        .find(|state| state["account_id"] == json!(f.account))
        .expect("the account");
    assert_eq!(state["holdings"][0]["instrument_id"], json!(fund));
}

#[tokio::test]
async fn a_file_over_the_byte_cap_is_refused() {
    let f = fixture().await;
    let huge = "x".repeat(saffron_server::imports::MAX_CSV_BYTES + 1);

    let refused = preview(&f, &huge).await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"], json!("file_too_large"));
}

#[tokio::test]
async fn viewers_export_but_cannot_import() {
    let h = household().await;
    let id = portfolio(&h.app, &h.admin, "Home").await;
    let f = Fixture {
        app: h.app,
        cookie: h.admin,
        portfolio: id,
        account: 0,
        stock: 0,
    };
    book_steps::account(
        &f.app,
        &f.cookie,
        &f.portfolio,
        json!({ "name": "Brokerage", "kind": "book_entry" }),
    )
    .await;
    share(&f.app, &f.cookie, &f.portfolio, "member", "viewer").await;
    let csv = csv_text(&[
        header(),
        row([
            "Brokerage",
            "deposit",
            "2026-01-02",
            "",
            "",
            "",
            "100",
            "",
            "EUR",
            "",
            "",
        ]),
    ]);

    for path in ["/export.csv", "/export.json"] {
        let reply = f
            .app
            .get(&format!("/api/portfolios/{}{path}", f.portfolio))
            .cookie(&h.member)
            .send()
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
    }
    for path in ["/import", "/import/preview"] {
        let refused = f
            .app
            .post(&format!("/api/portfolios/{}{path}", f.portfolio))
            .cookie(&h.member)
            .json(json!({ "csv": csv, "columns": columns() }))
            .send()
            .await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "{path}");
    }

    share(&f.app, &f.cookie, &f.portfolio, "member", "editor").await;
    let allowed = f
        .app
        .post(&format!("/api/portfolios/{}/import", f.portfolio))
        .cookie(&h.member)
        .json(json!({ "csv": csv, "columns": columns() }))
        .send()
        .await;
    assert_eq!(allowed.status, StatusCode::OK, "{:?}", allowed.json());
}
