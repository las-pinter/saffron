//! Generated portfolios: what must always hold of a book's lots.

use jiff::civil::Date;
use proptest::prelude::*;
use rust_decimal::Decimal;
use saffron_core::book::{Account, AccountKind, Book, BookError, Kind, Transaction, read};

const EUR: &str = "EUR";

fn date_of(day: usize) -> Date {
    format!("2026-01-{day:02}")
        .parse()
        .expect("a generated date")
}

fn accounts() -> Vec<Account> {
    [1, 2]
        .into_iter()
        .map(|id| Account {
            id,
            kind: AccountKind::BookEntry,
            currency: EUR.to_owned(),
        })
        .collect()
}

/// A buy or a sell: units, price, and fee in the account's currency.
#[derive(Debug, Clone, Copy)]
struct Trade {
    account: i64,
    instrument: i64,
    quantity: u32,
    price: u32,
    fee: u32,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Buy(Trade),
    Sell(Trade),
    Split {
        account: i64,
        instrument: i64,
        ratio: u32,
    },
    Transfer {
        from: i64,
        to: i64,
        instrument: i64,
        quantity: u32,
    },
}

fn trade_strategy() -> impl Strategy<Value = Trade> {
    (1i64..=2, 1i64..=2, 1u32..=20, 1u32..=200, 0u32..=5).prop_map(
        |(account, instrument, quantity, price, fee)| Trade {
            account,
            instrument,
            quantity,
            price,
            fee,
        },
    )
}

fn step_strategy() -> impl Strategy<Value = Step> {
    prop_oneof![
        trade_strategy().prop_map(Step::Buy),
        trade_strategy().prop_map(Step::Sell),
        (1i64..=2, 1i64..=2, 1u32..=4).prop_map(|(account, instrument, ratio)| Step::Split {
            account,
            instrument,
            ratio,
        }),
        (1i64..=2, 1i64..=2, 1i64..=2, 1u32..=20).prop_map(|(from, to, instrument, quantity)| {
            Step::Transfer {
                from,
                to,
                instrument,
                quantity,
            }
        }),
    ]
}

#[expect(
    clippy::too_many_arguments,
    reason = "the fixture mirrors the fields of a transaction"
)]
fn transaction(
    id: i64,
    account: i64,
    instrument: i64,
    kind: Kind,
    date: Date,
    quantity: Decimal,
    price: Decimal,
    fee: Decimal,
) -> Transaction {
    Transaction {
        id,
        account_id: account,
        instrument_id: Some(instrument),
        kind,
        date,
        quantity: Some(quantity),
        price: Some(price),
        amount: None,
        fee,
        currency: EUR.to_owned(),
        fx_rate: None,
    }
}

/// Builds the raw transactions of `steps`, with a small set of days so same-day
/// mixtures occur. They need not form a valid book; the property accepts a
/// refusal for an oversale.
fn build(steps: &[Step]) -> Vec<Transaction> {
    let mut transactions = Vec::new();
    let mut next_id = 1;
    for (index, step) in steps.iter().enumerate() {
        let date = date_of(1 + index % 7);
        match *step {
            Step::Buy(trade) => {
                transactions.push(transaction(
                    next_id,
                    trade.account,
                    trade.instrument,
                    Kind::Buy,
                    date,
                    Decimal::from(trade.quantity),
                    Decimal::from(trade.price),
                    Decimal::from(trade.fee),
                ));
                next_id += 1;
            }
            Step::Sell(trade) => {
                transactions.push(transaction(
                    next_id,
                    trade.account,
                    trade.instrument,
                    Kind::Sell,
                    date,
                    Decimal::from(trade.quantity),
                    Decimal::from(trade.price),
                    Decimal::from(trade.fee),
                ));
                next_id += 1;
            }
            Step::Split {
                account,
                instrument,
                ratio,
            } => {
                transactions.push(transaction(
                    next_id,
                    account,
                    instrument,
                    Kind::Split,
                    date,
                    Decimal::from(ratio),
                    Decimal::ZERO,
                    Decimal::ZERO,
                ));
                next_id += 1;
            }
            Step::Transfer {
                from,
                to,
                instrument,
                quantity,
            } => {
                transactions.push(transaction(
                    next_id,
                    from,
                    instrument,
                    Kind::TransferOut,
                    date,
                    Decimal::from(quantity),
                    Decimal::ZERO,
                    Decimal::ZERO,
                ));
                transactions.push(transaction(
                    next_id + 1,
                    to,
                    instrument,
                    Kind::TransferIn,
                    date,
                    Decimal::from(quantity),
                    Decimal::ONE,
                    Decimal::ZERO,
                ));
                next_id += 2;
            }
        }
    }
    transactions
}

fn cents(value: Decimal) -> Decimal {
    value.round_dp(2)
}

proptest! {
    #[test]
    fn no_lot_ever_holds_a_negative_quantity(steps in prop::collection::vec(step_strategy(), 1..=40)) {
        let transactions = build(&steps);
        match read(&accounts(), &transactions) {
            // Asking for more than the account holds is a refused request, not a
            // book with a short position.
            Err(BookError::Oversold { .. }) => {}
            Err(other) => prop_assert!(false, "unexpected refusal: {other}"),
            Ok(book) => {
                for state in book.accounts.values() {
                    for holding in state.holdings.values() {
                        let mut quantity = Decimal::ZERO;
                        let mut basis = Decimal::ZERO;
                        for lot in &holding.lots {
                            prop_assert!(lot.quantity > Decimal::ZERO, "a lot holds {}", lot.quantity);
                            quantity += lot.quantity;
                            basis += lot.quantity * lot.unit_cost;
                        }
                        prop_assert_eq!(holding.quantity, quantity);
                        prop_assert_eq!(holding.cost_basis, basis);
                        prop_assert!(holding.quantity >= Decimal::ZERO);
                    }
                }
            }
        }
    }

    #[test]
    fn a_sale_books_the_result_of_the_oldest_lots(
        first_quantity in 1u32..=20,
        first_price in 1u32..=200,
        first_fee in 0u32..=5,
        second_quantity in 1u32..=20,
        second_price in 1u32..=200,
        second_fee in 0u32..=5,
        sold in 0u32..=40,
        sale_price in 1u32..=200,
        sale_fee in 0u32..=5,
    ) {
        let total = Decimal::from(first_quantity + second_quantity);
        let sold = Decimal::from(sold).min(total);
        prop_assume!(!sold.is_zero());
        let first = Decimal::from(first_quantity);
        let second = Decimal::from(second_quantity);
        let unit_first = Decimal::from(first_price) + Decimal::from(first_fee) / first;
        let unit_second = Decimal::from(second_price) + Decimal::from(second_fee) / second;
        let net = Decimal::from(sale_price) - Decimal::from(sale_fee) / sold;
        let from_first = sold.min(first);
        let from_second = sold - from_first;
        let expected = (net - unit_first) * from_first + (net - unit_second) * from_second;

        let transactions = vec![
            transaction(1, 1, 100, Kind::Buy, date_of(1), first, Decimal::from(first_price), Decimal::from(first_fee)),
            transaction(2, 1, 100, Kind::Buy, date_of(2), second, Decimal::from(second_price), Decimal::from(second_fee)),
            transaction(3, 1, 100, Kind::Sell, date_of(3), sold, Decimal::from(sale_price), Decimal::from(sale_fee)),
        ];

        let book = read(&accounts(), &transactions).expect("a valid sale");
        let realized = book.accounts.get(&1).expect("the account").realized;

        prop_assert_eq!(cents(realized), cents(expected));
    }

    #[test]
    fn a_paired_transfer_keeps_the_portfolio_basis(
        quantity in 1u32..=20,
        moved in 0u32..=20,
        price in 1u32..=200,
    ) {
        let moved = Decimal::from(moved).min(Decimal::from(quantity));
        let quantity = Decimal::from(quantity);
        let price = Decimal::from(price);
        let bought = vec![transaction(1, 1, 100, Kind::Buy, date_of(1), quantity, price, Decimal::ZERO)];
        let mut transferred = bought.clone();
        if !moved.is_zero() {
            transferred.push(transaction(2, 1, 100, Kind::TransferOut, date_of(2), moved, Decimal::ZERO, Decimal::ZERO));
            transferred.push(transaction(3, 2, 100, Kind::TransferIn, date_of(2), moved, Decimal::ONE, Decimal::ZERO));
        }

        let before = read(&accounts(), &bought).expect("before");
        let after = read(&accounts(), &transferred).expect("after");
        let total = |book: &Book| -> (Decimal, Decimal) {
            book.accounts
                .values()
                .flat_map(|state| state.holdings.values())
                .fold((Decimal::ZERO, Decimal::ZERO), |(quantity, basis), holding| {
                    (quantity + holding.quantity, basis + holding.cost_basis)
                })
        };

        prop_assert_eq!(total(&after), total(&before));
        let receiver = after
            .accounts
            .get(&2)
            .expect("the receiving account")
            .holdings
            .get(&100);
        if let Some(receiver) = receiver {
            for lot in &receiver.lots {
                prop_assert_eq!(lot.unit_cost, price);
            }
        }
    }
}
