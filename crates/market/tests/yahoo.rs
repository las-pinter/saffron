//! The Yahoo client against recorded responses.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use saffron_core::instrument::InstrumentKind;
use saffron_market::{Listing, MarketError, SearchHit, mock::MockYahoo, yahoo::Yahoo};

#[tokio::test]
async fn an_op_fund_isin_finds_its_yahoo_listing() {
    let yahoo = MockYahoo::start().await;

    let hits = yahoo.client().expect("client").search("FI4000315700").await;

    assert_eq!(
        hits.expect("hits"),
        [SearchHit {
            symbol: "0P0001DHRJ.F".to_owned(),
            name: "OP-Suomi Indeksi B".to_owned(),
            kind: InstrumentKind::Fund,
            exchange: Some("Frankfurt".to_owned()),
        }]
    );
}

#[tokio::test]
async fn the_op_fund_listing_is_valued_in_euros() {
    let yahoo = MockYahoo::start().await;

    let listing = yahoo
        .client()
        .expect("client")
        .details("0P0001DHRJ.F")
        .await;

    assert_eq!(
        listing.expect("listing"),
        Listing {
            symbol: "0P0001DHRJ.F".to_owned(),
            name: "OP-Suomi Indeksi B".to_owned(),
            kind: InstrumentKind::Fund,
            currency: "EUR".to_owned(),
            quote_currency: "EUR".to_owned(),
            exchange: Some("Frankfurt".to_owned()),
        }
    );
}

#[tokio::test]
async fn a_name_search_finds_the_helsinki_listing() {
    let yahoo = MockYahoo::start().await;

    let hits = yahoo
        .client()
        .expect("client")
        .search("nokia")
        .await
        .expect("hits");

    let helsinki = hits.iter().find(|hit| hit.symbol == "NOKIA.HE");
    assert_eq!(
        helsinki.map(|hit| (hit.name.as_str(), hit.kind)),
        Some(("Nokia Oyj", InstrumentKind::Stock))
    );
}

#[tokio::test]
async fn an_etf_isin_finds_its_listing() {
    let yahoo = MockYahoo::start().await;

    let hits = yahoo
        .client()
        .expect("client")
        .search("IE00B4L5Y983")
        .await
        .expect("hits");

    assert_eq!(
        hits.iter()
            .map(|hit| (hit.symbol.as_str(), hit.kind))
            .collect::<Vec<_>>(),
        [("IWDA.L", InstrumentKind::Etf)]
    );
}

#[tokio::test]
async fn an_index_is_left_out_of_the_results() {
    let yahoo = MockYahoo::start().await;

    let hits = yahoo.client().expect("client").search("omxh25").await;

    assert_eq!(hits.expect("hits"), []);
}

#[tokio::test]
async fn an_unknown_symbol_is_not_found() {
    let yahoo = MockYahoo::start().await;

    let listing = yahoo.client().expect("client").details("NOSUCH123XX").await;

    assert!(matches!(listing, Err(MarketError::NotFound)), "{listing:?}");
}

#[tokio::test]
async fn a_429_is_reported_as_rate_limited() {
    let yahoo = MockYahoo::start().await;
    yahoo.fail_with(StatusCode::TOO_MANY_REQUESTS);

    let hits = yahoo.client().expect("client").search("nokia").await;

    assert!(matches!(hits, Err(MarketError::RateLimited)), "{hits:?}");
}

#[tokio::test]
async fn a_server_error_is_reported_as_unavailable() {
    let yahoo = MockYahoo::start().await;
    yahoo.fail_with(StatusCode::BAD_GATEWAY);

    let hits = yahoo.client().expect("client").search("nokia").await;

    assert!(matches!(hits, Err(MarketError::Unavailable(_))), "{hits:?}");
}

#[tokio::test]
async fn a_body_that_is_not_json_is_a_bad_response() {
    let yahoo = MockYahoo::start().await;
    yahoo.fail_with(StatusCode::NO_CONTENT);

    let hits = yahoo.client().expect("client").search("nokia").await;

    assert!(matches!(hits, Err(MarketError::BadResponse(_))), "{hits:?}");
}

#[tokio::test]
async fn an_unreachable_provider_is_unavailable() {
    let client = Yahoo::new("http://127.0.0.1:1", Duration::ZERO).expect("client");

    let hits = client.search("nokia").await;

    assert!(matches!(hits, Err(MarketError::Unavailable(_))), "{hits:?}");
}

#[tokio::test]
async fn requests_are_spaced_by_the_gap() {
    let yahoo = MockYahoo::start().await;
    let gap = Duration::from_millis(300);
    let client = Yahoo::new(&yahoo.url(), gap).expect("client");
    let started = Instant::now();

    for _ in 0..3 {
        client.search("nokia").await.expect("hits");
    }

    assert!(started.elapsed() >= gap * 2, "{:?}", started.elapsed());
}

#[tokio::test]
async fn an_index_listing_is_unsupported() {
    let yahoo = MockYahoo::start().await;

    let listing = yahoo.client().expect("client").details("^OMXH25").await;

    assert!(
        matches!(&listing, Err(MarketError::Unsupported(kind)) if kind == "INDEX"),
        "{listing:?}"
    );
}

#[tokio::test]
async fn a_burst_beyond_the_waiting_limit_is_refused_as_busy() {
    let yahoo = MockYahoo::start().await;
    let client = Arc::new(Yahoo::new(&yahoo.url(), Duration::from_millis(200)).expect("client"));
    let mut burst = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let client = Arc::clone(&client);
        burst.spawn(async move { client.search("nokia").await });
    }

    let outcomes = burst.join_all().await;

    let busy = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Err(MarketError::Busy)))
        .count();
    let answered = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    assert_eq!((answered, busy), (5, 3));
}
