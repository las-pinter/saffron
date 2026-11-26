//! Instrument kinds and ISINs.

use saffron_core::instrument::{InstrumentKind, Isin};

#[test]
fn real_isins_are_accepted() {
    let codes = [
        "FI4000315700",
        "IE00B4L5Y983",
        "US0378331005",
        "FI0009000681",
    ];

    let parsed: Vec<_> = codes.iter().map(|code| Isin::parse(code)).collect();

    assert!(parsed.iter().all(Option::is_some), "{parsed:?}");
}

#[test]
fn an_isin_is_read_in_upper_case_without_space() {
    let isin = Isin::parse("  fi4000315700 ");

    assert_eq!(isin.as_ref().map(Isin::as_str), Some("FI4000315700"));
}

#[test]
fn a_wrong_check_digit_is_refused() {
    assert_eq!(Isin::parse("FI4000315701"), None);
}

#[test]
fn a_wrong_shape_is_refused() {
    let codes = [
        "FI400031570",
        "FI40003157000",
        "1I4000315700",
        "FI400031570X",
        "FI40003-5700",
    ];

    let parsed: Vec<_> = codes.iter().map(|code| Isin::parse(code)).collect();

    assert!(parsed.iter().all(Option::is_none), "{parsed:?}");
}

#[test]
fn a_kind_reads_back_from_its_name() {
    let kinds = [
        InstrumentKind::Stock,
        InstrumentKind::Etf,
        InstrumentKind::Fund,
        InstrumentKind::Bond,
        InstrumentKind::Crypto,
        InstrumentKind::Other,
    ];

    let round_trip: Vec<_> = kinds
        .iter()
        .map(|kind| InstrumentKind::parse(kind.as_str()))
        .collect();

    assert_eq!(round_trip, kinds.map(Some));
}

#[test]
fn an_unknown_kind_name_is_refused() {
    assert_eq!(InstrumentKind::parse("Stock"), None);
}

#[test]
fn kinds_use_the_names_the_database_accepts() {
    let kinds = [
        InstrumentKind::Stock,
        InstrumentKind::Etf,
        InstrumentKind::Fund,
        InstrumentKind::Bond,
        InstrumentKind::Crypto,
        InstrumentKind::Other,
    ];

    let names = kinds.map(InstrumentKind::as_str);

    assert_eq!(names, ["stock", "etf", "fund", "bond", "crypto", "other"]);
}
