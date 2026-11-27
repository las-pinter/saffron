//! Price queries: one close per day, hand-entered prices kept, latest and history.

use std::str::FromStr;

use jiff::{
    Timestamp,
    civil::{Date, date},
};
use rust_decimal::Decimal;
use saffron_core::instrument::InstrumentKind;
use saffron_db::{
    instruments::{self, NewInstrument, NewListing, PriceListing},
    open_in_memory,
    prices::{self, MANUAL, Price},
};
use sqlx::SqlitePool;

fn at(text: &str) -> Timestamp {
    text.parse().expect("timestamp")
}

async fn db() -> SqlitePool {
    open_in_memory().await.expect("in-memory database")
}

async fn instrument(pool: &SqlitePool, symbol: &str, kind: InstrumentKind) -> i64 {
    let new = NewInstrument {
        kind,
        name: symbol,
        isin: None,
        currency: "EUR",
    };
    let listing = NewListing {
        provider: "yahoo",
        symbol,
        exchange: None,
        currency: "EUR",
        timezone: None,
    };
    instruments::create(pool, new, listing, at("2026-11-26T12:00:00Z"))
        .await
        .expect("create")
}

fn price(instrument_id: i64, day: Date, close: &str, source: &str) -> Price {
    Price {
        instrument_id,
        date: day,
        close: Decimal::from_str(close).expect("decimal"),
        currency: "EUR".to_owned(),
        source: source.to_owned(),
        as_of: at("2026-11-26T15:00:00Z"),
        checked_at: at("2026-11-26T15:05:00Z"),
    }
}

#[tokio::test]
async fn a_price_reads_back_as_stored() {
    let pool = db().await;
    let id = instrument(&pool, "NOKIA.HE", InstrumentKind::Stock).await;
    let stored = price(id, date(2026, 11, 26), "4.215", "yahoo");

    prices::record(&pool, &stored).await.expect("record");

    assert_eq!(
        prices::latest(&pool, id).await.expect("latest"),
        Some(stored)
    );
}

#[tokio::test]
async fn a_later_quote_replaces_the_day() {
    let pool = db().await;
    let id = instrument(&pool, "NOKIA.HE", InstrumentKind::Stock).await;
    prices::record(&pool, &price(id, date(2026, 11, 26), "4.2", "yahoo"))
        .await
        .expect("record");

    prices::record(&pool, &price(id, date(2026, 11, 26), "4.3", "yahoo"))
        .await
        .expect("record again");

    let history = prices::history(&pool, id, date(2026, 1, 1))
        .await
        .expect("history");
    assert_eq!(
        history
            .iter()
            .map(|price| price.close.to_string())
            .collect::<Vec<_>>(),
        ["4.3"]
    );
}

#[tokio::test]
async fn a_provider_does_not_replace_a_manual_price() {
    let pool = db().await;
    let id = instrument(&pool, "0P0001DHRJ.F", InstrumentKind::Fund).await;
    let manual = price(id, date(2026, 11, 25), "136.87", MANUAL);
    prices::record(&pool, &manual).await.expect("manual");

    let stored = prices::record(&pool, &price(id, date(2026, 11, 25), "999", "ft"))
        .await
        .expect("provider");

    assert_eq!(
        (stored, prices::latest(&pool, id).await.expect("latest")),
        (false, Some(manual))
    );
}

#[tokio::test]
async fn a_manual_price_replaces_any_price() {
    let pool = db().await;
    let id = instrument(&pool, "0P0001DHRJ.F", InstrumentKind::Fund).await;
    prices::record(&pool, &price(id, date(2026, 11, 25), "999", "ft"))
        .await
        .expect("provider");
    let manual = price(id, date(2026, 11, 25), "136.87", MANUAL);

    let stored = prices::record(&pool, &manual).await.expect("manual");

    assert_eq!(
        (stored, prices::latest(&pool, id).await.expect("latest")),
        (true, Some(manual))
    );
}

#[tokio::test]
async fn the_latest_price_is_the_newest_day() {
    let pool = db().await;
    let id = instrument(&pool, "NOKIA.HE", InstrumentKind::Stock).await;
    for (day, close) in [(25, "2"), (26, "3"), (24, "1")] {
        prices::record(&pool, &price(id, date(2026, 11, day), close, "yahoo"))
            .await
            .expect("record");
    }

    let latest = prices::latest(&pool, id)
        .await
        .expect("latest")
        .expect("some");

    assert_eq!(
        (latest.date, latest.close.to_string()),
        (date(2026, 11, 26), "3".to_owned())
    );
}

#[tokio::test]
async fn history_starts_at_its_date_and_runs_oldest_first() {
    let pool = db().await;
    let id = instrument(&pool, "NOKIA.HE", InstrumentKind::Stock).await;
    let other = instrument(&pool, "TYRES.HE", InstrumentKind::Stock).await;
    let batch = [
        price(id, date(2026, 11, 26), "3", "yahoo"),
        price(id, date(2026, 11, 23), "0", "yahoo"),
        price(id, date(2026, 11, 25), "2", "yahoo"),
        price(other, date(2026, 11, 25), "9", "yahoo"),
    ];
    prices::record_all(&pool, &batch).await.expect("record all");

    let history = prices::history(&pool, id, date(2026, 11, 24))
        .await
        .expect("history");

    assert_eq!(
        history.iter().map(|price| price.date).collect::<Vec<_>>(),
        [date(2026, 11, 25), date(2026, 11, 26)]
    );
}

#[tokio::test]
async fn a_batch_counts_what_it_stored() {
    let pool = db().await;
    let id = instrument(&pool, "0P0001DHRJ.F", InstrumentKind::Fund).await;
    prices::record(&pool, &price(id, date(2026, 11, 25), "136.87", MANUAL))
        .await
        .expect("manual");
    let batch = [
        price(id, date(2026, 11, 24), "138.75", "ft"),
        price(id, date(2026, 11, 25), "999", "ft"),
    ];

    let stored = prices::record_all(&pool, &batch).await.expect("record all");

    assert_eq!(stored, 1);
}

#[tokio::test]
async fn price_listings_take_one_listing_per_instrument_at_the_provider() {
    let pool = db().await;
    let id = instrument(&pool, "0P0001DHRJ.F", InstrumentKind::Fund).await;
    let second = NewListing {
        provider: "yahoo",
        symbol: "0P0001DHRJ.BE",
        exchange: None,
        currency: "EUR",
        timezone: None,
    };
    instruments::add_listing(&pool, id, second)
        .await
        .expect("second");
    let elsewhere = NewListing {
        provider: "ft",
        symbol: "FI4000315700:EUR",
        ..second
    };
    instruments::add_listing(&pool, id, elsewhere)
        .await
        .expect("ft");
    instruments::set_timezone(&pool, "yahoo", "0P0001DHRJ.F", "Europe/Berlin")
        .await
        .expect("timezone");

    let listings = instruments::price_listings(&pool, "yahoo")
        .await
        .expect("listings");

    assert_eq!(
        listings,
        [PriceListing {
            instrument_id: id,
            kind: InstrumentKind::Fund,
            isin: None,
            symbol: "0P0001DHRJ.F".to_owned(),
            timezone: Some("Europe/Berlin".to_owned()),
            currency: "EUR".to_owned(),
            instrument_currency: "EUR".to_owned(),
            last_attempt: None,
            history_attempt: None,
            history_done: None,
            timezone_attempt: None,
        }]
    );
}

#[tokio::test]
async fn a_legacy_row_without_a_checked_at_reads_as_its_as_of() {
    let pool = db().await;
    let id = instrument(&pool, "NOKIA.HE", InstrumentKind::Stock).await;
    sqlx::query(
        "INSERT INTO prices (instrument_id, date, close, currency, source, as_of)
         VALUES (?, '2026-11-25', '6.5', 'EUR', 'yahoo', '2026-11-25T15:00:00Z')",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("insert");
    let as_of = at("2026-11-25T15:00:00Z");

    let latest = prices::latest(&pool, id)
        .await
        .expect("latest")
        .expect("price");
    let marks = prices::latest_marks(&pool).await.expect("marks");

    assert_eq!(
        (latest.checked_at, marks.get(&id).copied()),
        (as_of, Some((as_of, as_of)))
    );
}
