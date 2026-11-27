//! The live Yahoo and ECB endpoints. These call the internet, so they only run on
//! request: `just live`.

use saffron_core::instrument::InstrumentKind;
use saffron_market::{ecb::Ecb, yahoo::Yahoo};

#[tokio::test]
#[ignore = "calls Yahoo Finance; run with `just live`"]
async fn the_op_fund_isin_resolves_on_yahoo() {
    let yahoo = Yahoo::live().expect("client");

    let hits = yahoo.search("FI4000315700").await.expect("search");
    let listing = yahoo.details("0P0001DHRJ.F").await.expect("details");

    assert!(
        hits.iter().any(|hit| hit.symbol == "0P0001DHRJ.F"),
        "{hits:?}"
    );
    assert_eq!(
        (
            listing.name.as_str(),
            listing.currency.as_str(),
            listing.kind
        ),
        ("OP-Suomi Indeksi B", "EUR", InstrumentKind::Fund)
    );
}

#[tokio::test]
#[ignore = "calls the ECB; run with `just live`"]
async fn the_ecb_serves_rates_and_history() {
    let ecb = Ecb::live().expect("client");

    let day = ecb.daily().await.expect("daily rates");
    let history = ecb.history().await.expect("history");

    assert!(!day.per_eur.is_empty(), "a currency on the newest day");
    assert!(
        history.len() > 1000,
        "the archive holds years of days ({})",
        history.len()
    );
}

#[tokio::test]
#[ignore = "calls Yahoo Finance; run with `just live`"]
async fn the_op_fund_answers_with_a_quote_and_history() {
    let yahoo = Yahoo::live().expect("client");

    let spots = yahoo.quotes(&["0P0001DHRJ.F", "NOKIA.HE"]).await;
    let history = yahoo
        .history(
            "NOKIA.HE",
            jiff::civil::date(2026, 11, 1),
            jiff::civil::date(2026, 11, 26),
        )
        .await;

    assert!(
        spots.as_ref().is_ok_and(|spots| !spots.is_empty()),
        "a batched quote: {spots:?}"
    );
    assert!(
        history
            .as_ref()
            .is_ok_and(|history| !history.closes.is_empty()),
        "a month of daily closes: {history:?}"
    );
}
