//! Transactions the book must refuse, and the transfer pairing rules.

use std::str::FromStr;

use jiff::civil::Date;
use rust_decimal::Decimal;
use saffron_core::book::{Account, AccountKind, BookError, Kind, Transaction, read};

const STOCK: i64 = 100;
const EUR: &str = "EUR";
const USD: &str = "USD";

fn dec(text: &str) -> Decimal {
    Decimal::from_str(text).expect("a decimal fixture")
}

fn date(text: &str) -> Date {
    text.parse().expect("a date fixture")
}

fn tx(id: i64, account: i64, instrument: Option<i64>, kind: Kind, day: &str) -> Transaction {
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

fn buy(id: i64, account: i64, day: &str, quantity: &str, price: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        ..tx(id, account, Some(STOCK), Kind::Buy, day)
    }
}

fn sell(id: i64, account: i64, day: &str, quantity: &str, price: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        ..tx(id, account, Some(STOCK), Kind::Sell, day)
    }
}

fn transfer_out(id: i64, account: i64, day: &str, quantity: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        ..tx(id, account, Some(STOCK), Kind::TransferOut, day)
    }
}

fn transfer_in(id: i64, account: i64, day: &str, quantity: &str, price: &str) -> Transaction {
    Transaction {
        quantity: Some(dec(quantity)),
        price: Some(dec(price)),
        ..tx(id, account, Some(STOCK), Kind::TransferIn, day)
    }
}

fn accounts() -> Vec<Account> {
    [
        (1, AccountKind::BookEntry, EUR),
        (2, AccountKind::Fund, EUR),
        (3, AccountKind::BookEntry, USD),
        (4, AccountKind::Savings, EUR),
    ]
    .into_iter()
    .map(|(id, kind, currency)| Account {
        id,
        kind,
        currency: currency.to_owned(),
    })
    .collect()
}

fn error_of(transactions: &[Transaction]) -> BookError {
    read(&accounts(), transactions).expect_err("the book must be refused")
}

#[test]
fn selling_more_than_the_account_holds_is_refused() {
    let transactions = vec![
        buy(1, 1, "2026-01-05", "5", "10"),
        sell(2, 1, "2026-01-06", "6", "11"),
    ];

    let error = error_of(&transactions);

    assert_eq!(
        error,
        BookError::Oversold {
            transaction: 2,
            instrument: STOCK,
            wanted: dec("6"),
            held: dec("5"),
        }
    );
}

#[test]
fn a_transaction_naming_an_unknown_account_is_refused() {
    let transactions = vec![buy(1, 9, "2026-01-05", "5", "10")];

    let error = error_of(&transactions);

    assert_eq!(
        error,
        BookError::UnknownAccount {
            transaction: 1,
            account: 9,
        }
    );
}

#[test]
fn a_buy_without_an_instrument_is_refused() {
    let transactions = vec![Transaction {
        quantity: Some(dec("5")),
        price: Some(dec("10")),
        ..tx(1, 1, None, Kind::Buy, "2026-01-05")
    }];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("needs an instrument"))
    );
}

#[test]
fn a_fee_carrying_an_instrument_is_refused() {
    let transactions = vec![Transaction {
        amount: Some(dec("3")),
        ..tx(1, 1, Some(STOCK), Kind::Fee, "2026-01-05")
    }];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("does not carry an instrument"))
    );
}

#[test]
fn a_transaction_in_another_currency_needs_a_rate() {
    let transactions = vec![Transaction {
        quantity: Some(dec("5")),
        price: Some(dec("10")),
        currency: USD.to_owned(),
        ..tx(1, 1, Some(STOCK), Kind::Buy, "2026-01-05")
    }];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("fx_rate"))
    );
}

#[test]
fn a_rate_that_is_not_positive_is_refused() {
    let transactions = vec![Transaction {
        quantity: Some(dec("5")),
        price: Some(dec("10")),
        currency: USD.to_owned(),
        fx_rate: Some(Decimal::ZERO),
        ..tx(1, 1, Some(STOCK), Kind::Buy, "2026-01-05")
    }];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("fx_rate must be positive"))
    );
}

#[test]
fn a_negative_fee_is_refused() {
    let mut transaction = buy(1, 1, "2026-01-05", "5", "10");
    transaction.fee = dec("-1");

    let error = error_of(&[transaction]);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("cannot be negative"))
    );
}

#[test]
fn a_transfer_does_not_carry_a_fee() {
    let mut transaction = transfer_out(1, 1, "2026-01-05", "1");
    transaction.fee = dec("1");

    let error = error_of(&[transaction]);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("does not carry a fee"))
    );
}

#[test]
fn an_amount_beyond_the_range_is_refused() {
    let transactions = vec![buy(
        1,
        1,
        "2026-01-05",
        "79228162514264337593543950335",
        "2",
    )];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("out of range"))
    );
}

#[test]
fn a_savings_account_does_not_hold_securities() {
    let transactions = vec![buy(1, 4, "2026-01-05", "5", "10")];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("does not hold securities"))
    );
}

#[test]
fn a_deposit_without_an_amount_is_refused() {
    let transactions = vec![tx(1, 1, None, Kind::Deposit, "2026-01-05")];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("positive amount"))
    );
}

#[test]
fn a_split_ratio_must_be_positive() {
    let transactions = vec![Transaction {
        quantity: Some(dec("0")),
        ..tx(1, 1, Some(STOCK), Kind::Split, "2026-01-05")
    }];

    let error = error_of(&transactions);

    assert!(
        matches!(error, BookError::Invalid { transaction: 1, ref reason } if reason.contains("positive ratio"))
    );
}

#[test]
fn a_same_day_transfer_carries_the_original_cost() {
    let transactions = vec![
        buy(1, 1, "2026-01-05", "5", "10"),
        transfer_out(2, 1, "2026-01-06", "5"),
        transfer_in(3, 2, "2026-01-06", "5", "99"),
    ];

    let book = read(&accounts(), &transactions).expect("a paired transfer");

    let received = book
        .accounts
        .get(&2)
        .expect("the receiving account")
        .holdings
        .get(&STOCK)
        .expect("the received holding");
    assert_eq!(received.quantity, dec("5"));
    assert_eq!(received.lots[0].unit_cost, dec("10"), "the carried basis");
}

#[test]
fn a_transfer_in_on_another_day_starts_a_lot_at_its_own_price() {
    let transactions = vec![
        buy(1, 1, "2026-01-05", "5", "10"),
        transfer_out(2, 1, "2026-01-06", "5"),
        transfer_in(3, 2, "2026-01-07", "5", "99"),
    ];

    let book = read(&accounts(), &transactions).expect("an unpaired transfer");

    let received = book
        .accounts
        .get(&2)
        .expect("the receiving account")
        .holdings
        .get(&STOCK)
        .expect("the received holding");
    assert_eq!(received.lots[0].unit_cost, dec("99"), "its own price");
}

#[test]
fn a_transfer_between_two_currencies_is_refused() {
    let transactions = vec![
        buy(1, 1, "2026-01-05", "5", "10"),
        transfer_out(2, 1, "2026-01-06", "5"),
        Transaction {
            currency: USD.to_owned(),
            ..transfer_in(3, 3, "2026-01-06", "5", "11")
        },
    ];

    let error = error_of(&transactions);

    assert_eq!(error, BookError::TransferCurrency { transaction: 3 });
}
