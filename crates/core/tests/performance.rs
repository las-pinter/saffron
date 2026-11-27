//! Performance over fixture portfolios, against an independent reference.
//!
//! The expected TWR and XIRR values come from a separate daily-valuation
//! implementation of the same rules (deposits and withdrawals at the close,
//! XIRR act/365). They are asserted within one hundredth of a percentage point.

use std::collections::BTreeMap;
use std::str::FromStr;

use jiff::civil::Date;
use rust_decimal::Decimal;
use saffron_core::{
    book::{Account, AccountKind, Kind, Transaction},
    performance::{Close, Input, InstrumentInfo, Market, Report, Targets, read},
};

const EUR: &str = "EUR";
const USD: &str = "USD";
const STOCK: i64 = 1;
const FUND: i64 = 2;

fn dec(text: &str) -> Decimal {
    Decimal::from_str(text).expect("a decimal fixture")
}

fn date(text: &str) -> Date {
    text.parse().expect("a date fixture")
}

fn account(id: i64, currency: &str) -> Account {
    Account {
        id,
        kind: AccountKind::BookEntry,
        currency: currency.to_owned(),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the fixture mirrors the fields of a transaction"
)]
fn transaction(
    id: i64,
    account_id: i64,
    instrument_id: Option<i64>,
    kind: Kind,
    day: &str,
    quantity: Option<&str>,
    price: Option<&str>,
    amount: Option<&str>,
    currency: &str,
) -> Transaction {
    Transaction {
        id,
        account_id,
        instrument_id,
        kind,
        date: date(day),
        quantity: quantity.map(dec),
        price: price.map(dec),
        amount: amount.map(dec),
        fee: Decimal::ZERO,
        currency: currency.to_owned(),
        fx_rate: None,
    }
}

fn deposit(id: i64, account_id: i64, day: &str, amount: &str, currency: &str) -> Transaction {
    transaction(
        id,
        account_id,
        None,
        Kind::Deposit,
        day,
        None,
        None,
        Some(amount),
        currency,
    )
}

fn withdrawal(id: i64, account_id: i64, day: &str, amount: &str) -> Transaction {
    transaction(
        id,
        account_id,
        None,
        Kind::Withdrawal,
        day,
        None,
        None,
        Some(amount),
        EUR,
    )
}

fn buy(
    id: i64,
    account_id: i64,
    instrument: i64,
    day: &str,
    quantity: &str,
    price: &str,
    currency: &str,
) -> Transaction {
    transaction(
        id,
        account_id,
        Some(instrument),
        Kind::Buy,
        day,
        Some(quantity),
        Some(price),
        None,
        currency,
    )
}

fn closes(entries: &[(&str, &str)]) -> Vec<Close> {
    entries
        .iter()
        .map(|(day, value)| Close {
            date: date(day),
            value: dec(value),
        })
        .collect()
}

fn market(prices: &[(i64, &[(&str, &str)])], fx: &[(&str, &[(&str, &str)])]) -> Market {
    let mut market = Market::default();
    for (instrument, entries) in prices {
        market.prices.insert(*instrument, closes(entries));
    }
    for (instrument, currency) in [(STOCK, EUR), (FUND, USD)] {
        market.currencies.insert(instrument, currency.to_owned());
    }
    for (day, rates) in fx {
        market.fx.push((
            date(day),
            rates
                .iter()
                .map(|(code, rate)| ((*code).to_owned(), dec(rate)))
                .collect(),
        ));
    }
    market
}

fn info() -> BTreeMap<i64, InstrumentInfo> {
    [
        (
            STOCK,
            InstrumentInfo {
                asset_class: "equity".to_owned(),
                region: "europe".to_owned(),
            },
        ),
        (
            FUND,
            InstrumentInfo {
                asset_class: "bond".to_owned(),
                region: "north_america".to_owned(),
            },
        ),
    ]
    .into_iter()
    .collect()
}

fn report(
    accounts: &[Account],
    transactions: &[Transaction],
    market: &Market,
    from: &str,
    to: &str,
) -> Report {
    let instruments = info();
    read(&Input {
        accounts,
        transactions,
        base_currency: EUR,
        market,
        instruments: &instruments,
        benchmark: None,
        targets: None,
        from: date(from),
        to: date(to),
    })
    .expect("a valid book")
}

fn within(actual: Option<Decimal>, expected: &str) -> bool {
    actual.is_some_and(|actual| (actual - dec(expected)).abs() <= dec("0.01"))
}

#[test]
fn a_single_buy_is_ten_percent() {
    let accounts = [account(1, EUR)];
    let transactions = [
        deposit(1, 1, "2026-01-02", "1000", EUR),
        buy(2, 1, STOCK, "2026-01-05", "10", "50", EUR),
    ];
    let market = market(
        &[(
            STOCK,
            &[
                ("2026-01-05", "50"),
                ("2026-01-20", "55"),
                ("2026-02-01", "60"),
            ],
        )],
        &[],
    );

    let report = report(
        &accounts,
        &transactions,
        &market,
        "2026-01-02",
        "2026-02-10",
    );

    assert_eq!(report.points.len(), 40);
    assert_eq!(report.points[0].value, dec("1000"));
    assert_eq!(report.points[3].value, dec("1000"), "the day of the buy");
    assert_eq!(report.points[18].value, dec("1050"));
    assert_eq!(
        report.points.last().expect("a last point").value,
        dec("1100")
    );
    assert!(
        within(report.twr_percent, "10"),
        "TWR {:?}",
        report.twr_percent
    );
    assert!(
        within(report.xirr_percent, "144.001827"),
        "XIRR {:?}",
        report.xirr_percent
    );
    assert_eq!(report.unpriced_instruments, Vec::<i64>::new());
    assert_eq!(report.unpriced_currencies, Vec::<String>::new());
}

#[test]
fn a_deposit_and_a_withdrawal_are_not_returns() {
    let accounts = [account(1, EUR)];
    let transactions = [
        deposit(1, 1, "2026-01-02", "1000", EUR),
        buy(2, 1, STOCK, "2026-01-05", "20", "40", EUR),
        deposit(3, 1, "2026-01-20", "500", EUR),
        withdrawal(4, 1, "2026-02-10", "300"),
    ];
    let market = market(
        &[(
            STOCK,
            &[
                ("2026-01-05", "40"),
                ("2026-01-15", "45"),
                ("2026-02-01", "50"),
                ("2026-02-15", "48"),
            ],
        )],
        &[],
    );

    let report = report(
        &accounts,
        &transactions,
        &market,
        "2026-01-02",
        "2026-02-20",
    );

    assert_eq!(report.points[18].flow, dec("500"), "the deposit's day");
    assert_eq!(report.points[39].flow, dec("-300"), "the withdrawal's day");
    assert_eq!(report.points[39].value, dec("1400"));
    assert_eq!(
        report.points.last().expect("a last point").value,
        dec("1360")
    );
    assert!(
        within(report.twr_percent, "13.535714"),
        "TWR {:?}",
        report.twr_percent
    );
    assert!(
        within(report.xirr_percent, "145.111932"),
        "XIRR {:?}",
        report.xirr_percent
    );
}

#[test]
fn a_foreign_holding_is_valued_through_the_euro() {
    let accounts = [account(1, EUR), account(2, USD)];
    let transactions = [
        deposit(1, 1, "2026-01-02", "1000", EUR),
        deposit(2, 2, "2026-01-02", "500", USD),
        buy(3, 2, FUND, "2026-01-05", "10", "30", USD),
    ];
    let market = market(
        &[(FUND, &[("2026-01-05", "30"), ("2026-02-01", "35")])],
        &[
            ("2026-01-02", &[("EUR", "1"), ("USD", "1.10")]),
            ("2026-01-20", &[("EUR", "1"), ("USD", "1.20")]),
            ("2026-02-01", &[("EUR", "1"), ("USD", "1.05")]),
            ("2026-02-10", &[("EUR", "1"), ("USD", "1.10")]),
        ],
    );

    let report = report(
        &accounts,
        &transactions,
        &market,
        "2026-01-02",
        "2026-02-10",
    );

    assert_eq!(
        report.points[0].value.round_dp(2),
        dec("1454.55"),
        "1000 EUR and 500 USD at 1.10"
    );
    assert_eq!(
        report.points.last().expect("a last point").value,
        dec("1500")
    );
    assert!(
        within(report.twr_percent, "3.125"),
        "TWR {:?}",
        report.twr_percent
    );
    assert!(
        within(report.xirr_percent, "33.374552"),
        "XIRR {:?}",
        report.xirr_percent
    );
}

#[test]
fn the_benchmark_is_normalized_to_one_hundred() {
    let accounts = [account(1, EUR)];
    let transactions = [deposit(1, 1, "2026-01-05", "100", EUR)];
    let market = market(
        &[(STOCK, &[("2026-01-05", "100"), ("2026-02-01", "110")])],
        &[],
    );
    let instruments = info();

    let report = read(&Input {
        accounts: &accounts,
        transactions: &transactions,
        base_currency: EUR,
        market: &market,
        instruments: &instruments,
        benchmark: Some(STOCK),
        targets: None,
        from: date("2026-01-05"),
        to: date("2026-02-10"),
    })
    .expect("a valid book");

    assert_eq!(report.benchmark.len(), 37);
    assert_eq!(report.benchmark[0].value, dec("100"));
    assert_eq!(
        report.benchmark.last().expect("a last point").value,
        dec("110")
    );
    assert_eq!(report.benchmark_percent, Some(dec("10")));
}

#[test]
fn allocation_groups_market_values_and_shows_drift() {
    let accounts = [account(1, EUR), account(2, USD)];
    let transactions = [
        deposit(1, 1, "2026-01-02", "100", EUR),
        deposit(2, 2, "2026-01-02", "110", USD),
        buy(3, 1, STOCK, "2026-01-05", "2", "50", EUR),
        buy(4, 2, FUND, "2026-01-05", "1", "30", USD),
    ];
    let market = market(
        &[
            (STOCK, &[("2026-01-05", "50")]),
            (FUND, &[("2026-01-05", "30")]),
        ],
        &[("2026-01-02", &[("EUR", "1"), ("USD", "1.10")])],
    );
    let instruments = info();
    let mut targets = Targets::default();
    targets.asset_class.insert("equity".to_owned(), dec("50"));
    targets.asset_class.insert("bond".to_owned(), dec("25"));
    targets.asset_class.insert("cash".to_owned(), dec("25"));
    targets.region.insert("europe".to_owned(), dec("50"));
    targets.region.insert("asia".to_owned(), dec("10"));

    let report = read(&Input {
        accounts: &accounts,
        transactions: &transactions,
        base_currency: EUR,
        market: &market,
        instruments: &instruments,
        benchmark: None,
        targets: Some(&targets),
        from: date("2026-01-02"),
        to: date("2026-01-10"),
    })
    .expect("a valid book");

    // Equity 100 EUR, bond 30/1.10 = 27.2727, cash 80/1.10 = 72.7273; the
    // EUR cash paid for the equity.
    let total = dec("200");
    let share = |key: &str| {
        report
            .allocation
            .asset_class
            .iter()
            .find(|share| share.key == key)
            .expect("a share")
            .clone()
    };
    let equity = share("equity");
    assert_eq!(equity.value, dec("100"));
    assert_eq!(
        equity.weight_percent,
        (dec("100") / total * dec("100")).round_dp(4)
    );
    assert_eq!(equity.target_percent, Some(dec("50")));
    assert_eq!(
        equity.drift_pp,
        Some((equity.weight_percent - dec("50")).round_dp(4))
    );
    let bond = share("bond");
    assert_eq!(bond.target_percent, Some(dec("25")));
    assert_eq!(
        bond.drift_pp,
        Some((bond.weight_percent - dec("25")).round_dp(4)),
        "a bond share below its target drifts down"
    );
    assert_eq!(share("cash").target_percent, Some(dec("25")));
    assert!(
        report
            .allocation
            .region
            .iter()
            .any(|share| share.key == "global")
    );
    let asia = report
        .allocation
        .region
        .iter()
        .find(|share| share.key == "asia")
        .expect("a target with no holding is still shown");
    assert_eq!(asia.value, Decimal::ZERO);
    assert_eq!(asia.weight_percent, Decimal::ZERO);
    assert_eq!(asia.drift_pp, Some(dec("-10")));
}

#[test]
fn a_holding_without_a_price_is_named_not_counted() {
    let accounts = [account(1, EUR)];
    let transactions = [
        deposit(1, 1, "2026-01-05", "100", EUR),
        buy(2, 1, STOCK, "2026-01-05", "5", "10", EUR),
    ];
    let market = Market::default();

    let report = report(
        &accounts,
        &transactions,
        &market,
        "2026-01-05",
        "2026-01-06",
    );

    assert_eq!(report.points[0].value, dec("50"), "the cash only");
    assert_eq!(report.unpriced_instruments, [STOCK]);
}

#[test]
fn a_foreign_flow_uses_the_rate_the_book_applied() {
    let accounts = [account(1, USD)];
    let mut deposit = deposit(1, 1, "2026-01-02", "500", EUR);
    deposit.fx_rate = Some(dec("1.2"));
    let transactions = [deposit];
    let market = market(&[], &[("2026-01-02", &[("EUR", "1"), ("USD", "1.10")])]);
    let instruments = info();

    let report = read(&Input {
        accounts: &accounts,
        transactions: &transactions,
        base_currency: EUR,
        market: &market,
        instruments: &instruments,
        benchmark: None,
        targets: None,
        from: date("2026-01-02"),
        to: date("2026-01-03"),
    })
    .expect("a valid book");

    // The book put 500 EUR × 1.2 = 600 USD into the account; at the ECB's 1.10
    // that is the day's value and its flow alike, so the day's return is zero.
    let value = dec("600") / dec("1.10");
    assert_eq!(report.points[0].value, value);
    assert_eq!(report.points[0].flow, value);
    assert_eq!(report.twr_percent, Some(Decimal::ZERO));
}

#[test]
fn a_fee_is_part_of_the_return_not_external_money() {
    let accounts = [account(1, EUR)];
    let mut deposit = deposit(1, 1, "2026-01-02", "1000", EUR);
    deposit.fee = dec("10");
    let transactions = [deposit];
    let market = Market::default();
    let instruments = info();

    let report = read(&Input {
        accounts: &accounts,
        transactions: &transactions,
        base_currency: EUR,
        market: &market,
        instruments: &instruments,
        benchmark: None,
        targets: None,
        from: date("2026-01-02"),
        to: date("2026-01-03"),
    })
    .expect("a valid book");

    // The account gained 990, the deposit less its fee; the fee is a cost of the
    // period, so the flow is the same 990 and the day earns nothing.
    assert_eq!(report.points[0].value, dec("990"));
    assert_eq!(report.points[0].flow, dec("990"));
    assert_eq!(report.twr_percent, Some(Decimal::ZERO));
}

#[test]
fn a_short_gain_still_has_an_xirr() {
    let accounts = [account(1, EUR)];
    let transactions = [
        deposit(1, 1, "2026-01-02", "100", EUR),
        buy(2, 1, STOCK, "2026-01-02", "10", "10", EUR),
    ];
    let market = market(
        &[(STOCK, &[("2026-01-02", "10"), ("2026-01-05", "10.2")])],
        &[],
    );
    let instruments = info();

    let report = read(&Input {
        accounts: &accounts,
        transactions: &transactions,
        base_currency: EUR,
        market: &market,
        instruments: &instruments,
        benchmark: None,
        targets: None,
        from: date("2026-01-02"),
        to: date("2026-01-05"),
    })
    .expect("a valid book");

    // 10 shares bought at 10 EUR are worth 10.2 EUR each three days later: a
    // 2 percent gain, or about 1012.64 percent a year.
    assert!(
        within(report.xirr_percent, "1012.6389"),
        "XIRR {:?}",
        report.xirr_percent
    );
}
