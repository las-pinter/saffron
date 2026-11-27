//! Instrument queries: instruments, their listings, and lookups by symbol or ISIN.

use jiff::Timestamp;
use saffron_core::instrument::InstrumentKind;
use saffron_db::{
    instruments::{self, Listing, NewInstrument, NewListing},
    is_unique_violation, open_in_memory,
};
use sqlx::SqlitePool;

fn now() -> Timestamp {
    "2026-11-26T12:00:00Z".parse().expect("timestamp")
}

async fn db() -> SqlitePool {
    open_in_memory().await.expect("in-memory database")
}

const FUND: NewInstrument<'static> = NewInstrument {
    kind: InstrumentKind::Fund,
    name: "OP-Suomi Indeksi B",
    isin: Some("FI4000315700"),
    currency: "EUR",
};

fn yahoo(symbol: &str) -> NewListing<'_> {
    NewListing {
        provider: "yahoo",
        symbol,
        exchange: Some("Frankfurt"),
        currency: "EUR",
        timezone: Some("Europe/Berlin"),
    }
}

async fn fund(pool: &SqlitePool) -> i64 {
    instruments::create(pool, FUND, yahoo("0P0001DHRJ.F"), now())
        .await
        .expect("create")
}

#[tokio::test]
async fn an_instrument_is_stored_with_its_primary_listing() {
    let pool = db().await;
    let id = fund(&pool).await;

    let stored = instruments::get(&pool, id)
        .await
        .expect("get")
        .expect("found");

    assert_eq!(
        (
            stored.kind,
            stored.name.as_str(),
            stored.isin.as_deref(),
            stored.currency.as_str(),
            stored.created_at.as_str(),
            stored.listings
        ),
        (
            InstrumentKind::Fund,
            "OP-Suomi Indeksi B",
            Some("FI4000315700"),
            "EUR",
            "2026-11-26T12:00:00Z",
            vec![Listing {
                provider: "yahoo".to_owned(),
                symbol: "0P0001DHRJ.F".to_owned(),
                exchange: Some("Frankfurt".to_owned()),
                currency: "EUR".to_owned(),
                is_primary: true,
                timezone: Some("Europe/Berlin".to_owned()),
            }]
        )
    );
}

#[tokio::test]
async fn a_second_listing_is_not_primary() {
    let pool = db().await;
    let id = fund(&pool).await;

    instruments::add_listing(&pool, id, yahoo("0P0001DHRJ.BE"))
        .await
        .expect("add listing");

    let listings = instruments::get(&pool, id)
        .await
        .expect("get")
        .expect("found")
        .listings;
    let primaries: Vec<_> = listings
        .iter()
        .map(|listing| (listing.symbol.as_str(), listing.is_primary))
        .collect();
    assert_eq!(
        primaries,
        [("0P0001DHRJ.F", true), ("0P0001DHRJ.BE", false)]
    );
}

#[tokio::test]
async fn a_listing_cannot_name_two_instruments() {
    let pool = db().await;
    fund(&pool).await;
    let other = NewInstrument { isin: None, ..FUND };

    let error = instruments::create(&pool, other, yahoo("0P0001DHRJ.F"), now())
        .await
        .expect_err("taken listing");

    assert!(is_unique_violation(&error), "{error}");
}

#[tokio::test]
async fn a_failed_listing_leaves_no_instrument_behind() {
    let pool = db().await;
    fund(&pool).await;
    let other = NewInstrument {
        isin: None,
        name: "Other",
        ..FUND
    };

    instruments::create(&pool, other, yahoo("0P0001DHRJ.F"), now())
        .await
        .expect_err("taken listing");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM instruments")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn an_isin_names_one_instrument() {
    let pool = db().await;
    fund(&pool).await;

    let error = instruments::create(&pool, FUND, yahoo("OTHER.F"), now())
        .await
        .expect_err("taken isin");

    assert!(is_unique_violation(&error), "{error}");
}

#[tokio::test]
async fn an_instrument_is_found_by_listing_and_by_isin() {
    let pool = db().await;
    let id = fund(&pool).await;

    let found = (
        instruments::id_by_listing(&pool, "yahoo", "0P0001DHRJ.F")
            .await
            .expect("by listing"),
        instruments::id_by_isin(&pool, "FI4000315700")
            .await
            .expect("by isin"),
        instruments::id_by_listing(&pool, "ft", "0P0001DHRJ.F")
            .await
            .expect("other provider"),
    );

    assert_eq!(found, (Some(id), Some(id), None));
}

#[tokio::test]
async fn known_symbols_are_picked_out_of_a_list() {
    let pool = db().await;
    let id = fund(&pool).await;
    let elsewhere = NewListing {
        provider: "ft",
        ..yahoo("NOKIA.HE")
    };
    let stock = NewInstrument { isin: None, ..FUND };
    instruments::create(&pool, stock, elsewhere, now())
        .await
        .expect("create");

    let known = instruments::known_symbols(&pool, "yahoo", &["NOKIA.HE", "0P0001DHRJ.F", "x\"y"])
        .await
        .expect("known");

    assert_eq!(known, [("0P0001DHRJ.F".to_owned(), id)]);
}

#[tokio::test]
async fn an_unknown_instrument_is_none() {
    let pool = db().await;
    fund(&pool).await;

    assert_eq!(instruments::get(&pool, 404).await.expect("get"), None);
}
