//! A forty-transaction portfolio read against an independently computed
//! reference.
//!
//! The expected figures come from a separate implementation of the same rules
//! (Python `Decimal`): fees go into a lot's cost, sales consume the oldest lots,
//! and a transfer carries its basis. They are asserted to the cent.

use std::str::FromStr;

use jiff::civil::Date;
use rust_decimal::Decimal;
use saffron_core::book::{
    Account, AccountKind, AccountState, Book, Holding, Kind, Transaction, read,
};

const BROKERAGE: i64 = 1;
const FUND_ACCOUNT: i64 = 2;
const STOCK: i64 = 100;
const FUND: i64 = 200;
const USD_STOCK: i64 = 300;
const EUR: &str = "EUR";
const USD: &str = "USD";

fn dec(text: &str) -> Decimal {
    Decimal::from_str(text).expect("a decimal fixture")
}

fn date(text: &str) -> Date {
    text.parse().expect("a date fixture")
}

fn cents(value: Decimal) -> Decimal {
    value.round_dp(2)
}

fn eur(id: i64, account: i64, instrument: Option<i64>, kind: Kind, day: &str) -> Transaction {
    Transaction {
        id,
        account_id: account,
        instrument_id: instrument,
        kind,
        date: date(day),
        quantity: None,
        price: None,
        amount: None,
        fee: Decimal::ZERO,
        currency: EUR.to_owned(),
        fx_rate: None,
    }
}

fn usd(id: i64, instrument: i64, kind: Kind, day: &str, fx: &str) -> Transaction {
    Transaction {
        currency: USD.to_owned(),
        fx_rate: Some(dec(fx)),
        ..eur(id, BROKERAGE, Some(instrument), kind, day)
    }
}

fn buy(
    id: i64,
    account: i64,
    instrument: i64,
    day: &str,
    quantity: &str,
    price: &str,
    fee: &str,
) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        fee: dec(fee),
        ..eur(id, account, Some(instrument), Kind::Buy, day)
    }
}

fn sell(
    id: i64,
    account: i64,
    instrument: i64,
    day: &str,
    quantity: &str,
    price: &str,
    fee: &str,
) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        fee: dec(fee),
        ..eur(id, account, Some(instrument), Kind::Sell, day)
    }
}

fn buy_usd(id: i64, day: &str, quantity: &str, price: &str, fee: &str, fx: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        fee: dec(fee),
        ..usd(id, USD_STOCK, Kind::Buy, day, fx)
    }
}

fn sell_usd(id: i64, day: &str, quantity: &str, price: &str, fee: &str, fx: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        fee: dec(fee),
        ..usd(id, USD_STOCK, Kind::Sell, day, fx)
    }
}

fn dividend_usd(
    id: i64,
    account: i64,
    instrument: i64,
    day: &str,
    amount: &str,
    fx: &str,
) -> Transaction {
    Transaction {
        amount: Some(dec(amount)),
        fee: Decimal::ONE,
        currency: USD.to_owned(),
        fx_rate: Some(dec(fx)),
        ..eur(id, account, Some(instrument), Kind::Dividend, day)
    }
}

fn dividend(
    id: i64,
    account: i64,
    instrument: i64,
    day: &str,
    amount: &str,
    fee: &str,
) -> Transaction {
    Transaction {
        amount: Some(dec(amount)),
        fee: dec(fee),
        ..eur(id, account, Some(instrument), Kind::Dividend, day)
    }
}

fn split(id: i64, account: i64, instrument: i64, day: &str, ratio: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(ratio)),
        ..eur(id, account, Some(instrument), Kind::Split, day)
    }
}

fn transfer_out(id: i64, account: i64, instrument: i64, day: &str, quantity: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        ..eur(id, account, Some(instrument), Kind::TransferOut, day)
    }
}

fn transfer_in(
    id: i64,
    account: i64,
    instrument: i64,
    day: &str,
    quantity: &str,
    price: &str,
) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        ..eur(id, account, Some(instrument), Kind::TransferIn, day)
    }
}

fn cash(id: i64, account: i64, kind: Kind, day: &str, amount: &str) -> Transaction {
    Transaction {
        amount: Some(dec(amount)),
        ..eur(id, account, None, kind, day)
    }
}

fn accounts() -> Vec<Account> {
    [
        (BROKERAGE, AccountKind::BookEntry),
        (FUND_ACCOUNT, AccountKind::Fund),
    ]
    .into_iter()
    .map(|(id, kind)| Account {
        id,
        kind,
        currency: EUR.to_owned(),
    })
    .collect()
}

/// The forty transactions of the fixture portfolio, in row order.
fn transactions() -> Vec<Transaction> {
    vec![
        cash(1, BROKERAGE, Kind::Deposit, "2026-01-02", "10000"),
        buy(2, BROKERAGE, STOCK, "2026-01-05", "10", "100", "5"),
        buy(3, BROKERAGE, STOCK, "2026-01-06", "10", "110", "5"),
        sell(4, BROKERAGE, STOCK, "2026-01-07", "15", "120", "6"),
        buy(5, BROKERAGE, FUND, "2026-01-08", "20", "50", "0"),
        dividend(6, BROKERAGE, STOCK, "2026-02-01", "25", "0"),
        cash(7, BROKERAGE, Kind::Fee, "2026-02-02", "3"),
        cash(8, FUND_ACCOUNT, Kind::Deposit, "2026-02-03", "5000"),
        buy(9, FUND_ACCOUNT, FUND, "2026-02-04", "10", "60", "2"),
        split(10, FUND_ACCOUNT, FUND, "2026-02-05", "2"),
        transfer_out(11, FUND_ACCOUNT, FUND, "2026-02-06", "5"),
        transfer_in(12, BROKERAGE, FUND, "2026-02-06", "5", "70"),
        sell(13, BROKERAGE, FUND, "2026-02-07", "10", "55", "0"),
        cash(14, BROKERAGE, Kind::Interest, "2026-02-08", "1.25"),
        cash(15, BROKERAGE, Kind::Withdrawal, "2026-02-09", "100"),
        cash(16, FUND_ACCOUNT, Kind::Fee, "2026-02-10", "1.5"),
        buy(17, BROKERAGE, STOCK, "2026-03-01", "4", "98.5", "1"),
        sell(18, BROKERAGE, STOCK, "2026-03-02", "2", "101.25", "0.5"),
        dividend(19, BROKERAGE, STOCK, "2026-03-03", "7.4", "0.4"),
        buy(20, FUND_ACCOUNT, FUND, "2026-03-04", "3", "28", "0.9"),
        cash(21, BROKERAGE, Kind::Deposit, "2026-03-05", "500"),
        buy_usd(22, "2026-03-06", "8", "25", "2", "0.9"),
        sell_usd(23, "2026-03-07", "3", "27", "1", "0.92"),
        dividend_usd(24, BROKERAGE, USD_STOCK, "2026-03-08", "12", "0.91"),
        cash(25, FUND_ACCOUNT, Kind::Interest, "2026-03-09", "0.4"),
        split(26, BROKERAGE, STOCK, "2026-04-01", "3"),
        sell(27, BROKERAGE, STOCK, "2026-04-02", "20", "35", "2"),
        sell(28, FUND_ACCOUNT, FUND, "2026-04-03", "8", "33", "1.2"),
        cash(29, BROKERAGE, Kind::Withdrawal, "2026-04-04", "250.75"),
        cash(30, FUND_ACCOUNT, Kind::Withdrawal, "2026-04-05", "1000"),
        buy(31, BROKERAGE, FUND, "2026-05-01", "6", "40", "1.8"),
        buy(32, FUND_ACCOUNT, FUND, "2026-05-02", "2", "39.5", "0.4"),
        sell(33, BROKERAGE, FUND, "2026-05-03", "7", "42", "1.4"),
        transfer_out(34, FUND_ACCOUNT, FUND, "2026-05-04", "4"),
        transfer_in(35, BROKERAGE, FUND, "2026-05-04", "4", "41"),
        dividend(36, BROKERAGE, STOCK, "2026-06-01", "5", "0"),
        cash(37, FUND_ACCOUNT, Kind::Fee, "2026-06-02", "2"),
        sell_usd(38, "2026-06-03", "5", "30", "0.5", "0.88"),
        cash(39, BROKERAGE, Kind::Interest, "2026-06-04", "2.5"),
        sell(40, FUND_ACCOUNT, FUND, "2026-06-05", "8", "45", "0"),
    ]
}

fn book(transactions: &[Transaction]) -> Book {
    read(&accounts(), transactions).expect("the fixture book is valid")
}

fn account(book: &Book, id: i64) -> &AccountState {
    book.accounts.get(&id).expect("an account in the book")
}

fn holding(book: &Book, account_id: i64, instrument: i64) -> &Holding {
    account(book, account_id)
        .holdings
        .get(&instrument)
        .expect("a holding in the account")
}

#[test]
fn the_forty_transaction_fixture_matches_the_reference_to_the_cent() {
    let transactions = transactions();
    assert_eq!(transactions.len(), 40);

    let book = book(&transactions);

    let brokerage = account(&book, BROKERAGE);
    assert_eq!(cents(brokerage.cash), dec("10010.17"), "brokerage cash");
    assert_eq!(
        cents(brokerage.realized),
        dec("237.88"),
        "brokerage realized"
    );
    let fund_account = account(&book, FUND_ACCOUNT);
    assert_eq!(cents(fund_account.cash), dec("3853.40"), "fund cash");
    assert_eq!(cents(fund_account.realized), dec("127.40"), "fund realized");
    assert!(
        fund_account.holdings.is_empty(),
        "the fund account holds nothing"
    );

    let stock = holding(&book, BROKERAGE, STOCK);
    assert_eq!(stock.quantity, dec("1"));
    assert_eq!(cents(stock.cost_basis), dec("32.92"));
    assert_eq!(
        cents(stock.average_cost().expect("a holding")),
        dec("32.92")
    );
    assert_eq!(stock.lots.len(), 1);
    assert_eq!(stock.lots[0].acquired, date("2026-03-01"));
    assert_eq!(stock.lots[0].quantity, dec("1"));
    assert_eq!(stock.lots[0].unit_cost.round_dp(4), dec("32.9167"));

    let fund = holding(&book, BROKERAGE, FUND);
    assert_eq!(fund.quantity, dec("18"));
    assert_eq!(cents(fund.cost_basis), dec("662.70"));
    assert_eq!(cents(fund.average_cost().expect("a holding")), dec("36.82"));
    // A transferred lot keeps the day it was first acquired (2026-02-04, the
    // fund account's buy) and takes its place among the lots by that day.
    let expected = [
        ("2026-01-08", "3", "50"),
        ("2026-02-04", "5", "30.1"),
        ("2026-02-04", "4", "30.1"),
        ("2026-05-01", "6", "40.3"),
    ];
    assert_eq!(fund.lots.len(), expected.len());
    for (lot, (day, quantity, unit_cost)) in fund.lots.iter().zip(expected) {
        assert_eq!(lot.acquired.to_string(), day);
        assert_eq!(lot.quantity, dec(quantity), "lot of {day}");
        assert_eq!(lot.unit_cost.round_dp(4), dec(unit_cost), "lot of {day}");
    }
}

#[test]
fn the_row_order_of_the_input_does_not_change_the_book() {
    let in_order = book(&transactions());
    let mut reversed = transactions();
    reversed.reverse();

    let from_reversed = book(&reversed);

    assert_eq!(from_reversed, in_order);
}

#[test]
fn the_book_is_read_by_day_not_by_row_number() {
    let canonical = book(&transactions());
    let mut renumbered = transactions();
    for transaction in &mut renumbered {
        transaction.id = 41 - transaction.id;
    }

    let from_renumbered = book(&renumbered);

    assert_eq!(from_renumbered, canonical);
}

#[test]
fn an_unpaired_transfer_in_starts_a_lot_at_its_own_price() {
    let transactions = vec![transfer_in(1, BROKERAGE, FUND, "2026-01-05", "5", "9")];

    let book = book(&transactions);

    let fund = holding(&book, BROKERAGE, FUND);
    assert_eq!(fund.quantity, dec("5"));
    assert_eq!(fund.lots[0].unit_cost, dec("9"));
    assert_eq!(account(&book, BROKERAGE).cash, Decimal::ZERO);
}

#[test]
fn an_older_transfer_in_takes_its_place_by_its_acquisition_day() {
    let transactions = vec![
        buy(1, BROKERAGE, STOCK, "2026-03-01", "1", "100", "0"),
        buy(2, FUND_ACCOUNT, STOCK, "2026-01-05", "1", "10", "0"),
        transfer_out(3, FUND_ACCOUNT, STOCK, "2026-04-01", "1"),
        transfer_in(4, BROKERAGE, STOCK, "2026-04-01", "1", "999"),
        sell(5, BROKERAGE, STOCK, "2026-04-02", "1", "50", "0"),
    ];

    let book = book(&transactions);

    let stock = holding(&book, BROKERAGE, STOCK);
    assert_eq!(
        cents(account(&book, BROKERAGE).realized),
        dec("40"),
        "the January lot is sold first"
    );
    assert_eq!(stock.quantity, dec("1"));
    assert_eq!(stock.lots[0].acquired, date("2026-03-01"));
    assert_eq!(stock.lots[0].unit_cost, dec("100"));
}

#[test]
fn two_same_day_transfers_each_keep_their_own_lots() {
    let transactions = vec![
        buy(1, FUND_ACCOUNT, STOCK, "2026-01-01", "1", "10", "0"),
        buy(2, FUND_ACCOUNT, STOCK, "2026-01-02", "1", "20", "0"),
        transfer_out(3, FUND_ACCOUNT, STOCK, "2026-01-05", "1"),
        transfer_out(4, FUND_ACCOUNT, STOCK, "2026-01-05", "1"),
        transfer_in(5, BROKERAGE, STOCK, "2026-01-05", "1", "99"),
        transfer_in(6, BROKERAGE, STOCK, "2026-01-05", "1", "99"),
    ];

    let book = book(&transactions);

    let stock = holding(&book, BROKERAGE, STOCK);
    assert_eq!(stock.quantity, dec("2"));
    let costs: Vec<Decimal> = stock.lots.iter().map(|lot| lot.unit_cost).collect();
    assert_eq!(
        costs,
        [dec("10"), dec("20")],
        "each lot carries its own cost"
    );
}

#[test]
fn three_same_day_transfers_pair_in_row_order() {
    let accounts = [
        Account {
            id: 1,
            kind: AccountKind::Fund,
            currency: EUR.to_owned(),
        },
        Account {
            id: 2,
            kind: AccountKind::BookEntry,
            currency: EUR.to_owned(),
        },
        Account {
            id: 3,
            kind: AccountKind::BookEntry,
            currency: EUR.to_owned(),
        },
    ];
    let transactions = vec![
        buy(1, 1, STOCK, "2026-01-01", "1", "10", "0"),
        buy(2, 1, STOCK, "2026-01-02", "1", "20", "0"),
        buy(3, 1, STOCK, "2026-01-03", "1", "30", "0"),
        transfer_out(4, 1, STOCK, "2026-01-05", "1"),
        transfer_out(5, 1, STOCK, "2026-01-05", "1"),
        transfer_out(6, 1, STOCK, "2026-01-05", "1"),
        transfer_in(7, 2, STOCK, "2026-01-05", "1", "99"),
        transfer_in(8, 3, STOCK, "2026-01-05", "1", "99"),
        transfer_in(9, 2, STOCK, "2026-01-05", "1", "99"),
    ];

    let book = read(&accounts, &transactions).expect("a valid book");

    let costs = |account: i64| -> Vec<Decimal> {
        book.accounts
            .get(&account)
            .expect("an account")
            .holdings
            .get(&STOCK)
            .expect("a holding")
            .lots
            .iter()
            .map(|lot| lot.unit_cost)
            .collect()
    };
    assert_eq!(costs(2), [dec("10"), dec("30")], "the first and third lots");
    assert_eq!(costs(3), [dec("20")], "the second lot");
}

#[test]
fn a_fee_reduces_every_kind_of_cash() {
    let with_fee = |transaction: Transaction, fee: &str| Transaction {
        fee: dec(fee),
        ..transaction
    };
    let transactions = vec![
        with_fee(cash(1, BROKERAGE, Kind::Deposit, "2026-01-02", "100"), "2"),
        with_fee(cash(2, BROKERAGE, Kind::Interest, "2026-01-03", "10"), "1"),
        with_fee(
            cash(3, BROKERAGE, Kind::Withdrawal, "2026-01-04", "50"),
            "1.5",
        ),
        with_fee(
            dividend(4, BROKERAGE, STOCK, "2026-01-05", "8", "0.5"),
            "0.5",
        ),
    ];

    let book = book(&transactions);

    assert_eq!(
        cents(account(&book, BROKERAGE).cash),
        dec("63"),
        "100-2 + 10-1 - (50+1.5) + (8-0.5)"
    );
}
