//! The transaction book: cash and holdings worked out from a portfolio's
//! transactions.
//!
//! Holdings are kept as lots bought over time. A sale consumes the oldest lots
//! first (FIFO) and books its realized result; an average-cost view is derived from
//! the lots that remain. Money is in the account's currency: a transaction in
//! another currency carries the rate to it.
//!
//! Conventions, all decided with the owner:
//! - A buy's fee is part of the lot's cost; a sale's fee comes off its proceeds.
//!   Every other cash kind nets its fee too: a deposit or interest loses it, a
//!   withdrawal adds it.
//! - A transfer moves lots with their original cost basis, so a portfolio is read
//!   as a whole and realized results stay true across accounts. A moved lot keeps
//!   its acquisition day and takes its place among the receiving lots by that day.
//! - A transfer out is paired with a transfer in of the same instrument on the
//!   same day, one out to one in, in transaction order; an unpaired transfer in
//!   starts a lot at its own price.
//! - A transfer stays in one currency: both accounts must hold the same currency.
//! - Only an account that holds securities may carry an instrument.
//! - Selling more than is held is an error, never a short position.

use std::collections::{BTreeMap, VecDeque};

use jiff::civil::Date;
use rust_decimal::Decimal;

/// A portfolio account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Row id.
    pub id: i64,
    /// What kind of account it is.
    pub kind: AccountKind,
    /// The ISO 4217 currency every amount of the account is kept in.
    pub currency: String,
}

/// The kinds of account a portfolio holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    /// Book-entry securities account.
    BookEntry,
    /// Equity savings account (osakesäästötili).
    EquitySavings,
    /// Fund account.
    Fund,
    /// Savings account.
    Savings,
    /// Cash account.
    Cash,
    /// Anything else.
    Other,
}

impl AccountKind {
    /// The API and database name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BookEntry => "book_entry",
            Self::EquitySavings => "equity_savings",
            Self::Fund => "fund",
            Self::Savings => "savings",
            Self::Cash => "cash",
            Self::Other => "other",
        }
    }

    /// The kind named `name`, or `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        const KINDS: [AccountKind; 6] = [
            AccountKind::BookEntry,
            AccountKind::EquitySavings,
            AccountKind::Fund,
            AccountKind::Savings,
            AccountKind::Cash,
            AccountKind::Other,
        ];
        KINDS.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Whether the account holds securities, as opposed to cash and savings only.
    #[must_use]
    pub const fn holds_securities(self) -> bool {
        !matches!(self, Self::Savings | Self::Cash)
    }
}

/// What a transaction did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// New units.
    Buy,
    /// Units sold.
    Sell,
    /// Cash paid out by a holding.
    Dividend,
    /// A charge on its own.
    Fee,
    /// Cash put in.
    Deposit,
    /// Cash taken out.
    Withdrawal,
    /// Cash earned on a balance.
    Interest,
    /// A split: the quantity is the new-per-old ratio.
    Split,
    /// Units arriving from another account.
    TransferIn,
    /// Units leaving for another account.
    TransferOut,
}

impl Kind {
    /// The API and database name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Dividend => "dividend",
            Self::Fee => "fee",
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Interest => "interest",
            Self::Split => "split",
            Self::TransferIn => "transfer_in",
            Self::TransferOut => "transfer_out",
        }
    }

    /// The kind named `name`, or `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        const KINDS: [Kind; 10] = [
            Kind::Buy,
            Kind::Sell,
            Kind::Dividend,
            Kind::Fee,
            Kind::Deposit,
            Kind::Withdrawal,
            Kind::Interest,
            Kind::Split,
            Kind::TransferIn,
            Kind::TransferOut,
        ];
        KINDS.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Whether the transaction names an instrument.
    #[must_use]
    pub const fn has_instrument(self) -> bool {
        !matches!(
            self,
            Self::Fee | Self::Deposit | Self::Withdrawal | Self::Interest
        )
    }

    /// Processing rank within a day: splits first, then units leaving, then units
    /// arriving, then everything else.
    const fn rank(self) -> u8 {
        match self {
            Self::Split => 0,
            Self::TransferOut => 1,
            Self::TransferIn => 2,
            _ => 3,
        }
    }
}

/// One transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// Row id; also breaks ties within a day.
    pub id: i64,
    /// The account it belongs to.
    pub account_id: i64,
    /// The security, for the kinds that name one.
    pub instrument_id: Option<i64>,
    /// What it did.
    pub kind: Kind,
    /// The day it happened.
    pub date: Date,
    /// Units; for a split, the new-per-old ratio.
    pub quantity: Option<Decimal>,
    /// Price per unit in [`Self::currency`].
    pub price: Option<Decimal>,
    /// Cash amount before fees, in [`Self::currency`].
    pub amount: Option<Decimal>,
    /// The charge, in [`Self::currency`].
    pub fee: Decimal,
    /// The ISO 4217 currency of the amounts.
    pub currency: String,
    /// Account-currency units per unit of [`Self::currency`], when they differ.
    pub fx_rate: Option<Decimal>,
}

/// Something that cannot be true of a book.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BookError {
    /// A transaction names an account the book does not hold.
    #[error("transaction {transaction} names unknown account {account}")]
    UnknownAccount {
        /// The transaction.
        transaction: i64,
        /// The account it names.
        account: i64,
    },
    /// A transaction lacks a field its kind needs, or carries a value that cannot
    /// be right.
    #[error("transaction {transaction}: {reason}")]
    Invalid {
        /// The transaction.
        transaction: i64,
        /// Why it is refused.
        reason: String,
    },
    /// A sale or transfer out asks for more units than the account holds.
    #[error("transaction {transaction} sells {wanted} {instrument} but the account holds {held}")]
    Oversold {
        /// The transaction.
        transaction: i64,
        /// The instrument.
        instrument: i64,
        /// Units asked for.
        wanted: Decimal,
        /// Units held.
        held: Decimal,
    },
    /// Two accounts in different currencies cannot receive moved lots.
    #[error("transaction {transaction}: a transfer must stay in one currency")]
    TransferCurrency {
        /// The transaction.
        transaction: i64,
    },
}

/// The state of one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountState {
    /// What kind of account it is.
    pub kind: AccountKind,
    /// Its currency.
    pub currency: String,
    /// Cash balance, in that currency.
    pub cash: Decimal,
    /// Realized result booked so far, in that currency.
    pub realized: Decimal,
    /// What it holds, by instrument.
    pub holdings: BTreeMap<i64, Holding>,
}

/// The units of one instrument an account holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    /// The remaining lots, oldest first.
    pub lots: Vec<Lot>,
    /// Units held, the sum of the lots.
    pub quantity: Decimal,
    /// What they cost, in the account currency.
    pub cost_basis: Decimal,
}

impl Holding {
    /// Cost per unit, the average-cost view; `None` when nothing is held.
    #[must_use]
    pub fn average_cost(&self) -> Option<Decimal> {
        (self.quantity > Decimal::ZERO).then(|| self.cost_basis / self.quantity)
    }
}

/// Units bought at one time, with what they cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lot {
    /// The day they were bought.
    pub acquired: Date,
    /// Units left in the lot.
    pub quantity: Decimal,
    /// Cost per unit in the account currency, fees included.
    pub unit_cost: Decimal,
}

/// The result of reading a portfolio's transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    /// The state of every account.
    pub accounts: BTreeMap<i64, AccountState>,
}

/// Reads the book of `accounts` from `transactions`.
///
/// Transactions are processed by day; within a day, splits run first, then units
/// leaving an account, then units arriving, then the rest in row order.
pub fn read(accounts: &[Account], transactions: &[Transaction]) -> Result<Book, BookError> {
    let mut states: BTreeMap<i64, AccountState> = accounts
        .iter()
        .map(|account| {
            (
                account.id,
                AccountState {
                    kind: account.kind,
                    currency: account.currency.clone(),
                    cash: Decimal::ZERO,
                    realized: Decimal::ZERO,
                    holdings: BTreeMap::new(),
                },
            )
        })
        .collect();
    let mut ordered: Vec<&Transaction> = transactions.iter().collect();
    ordered.sort_by_key(|transaction| (transaction.date, transaction.kind.rank(), transaction.id));
    let mut moving: BTreeMap<i64, Vec<Moving>> = BTreeMap::new();
    for transaction in ordered {
        apply(&mut states, &mut moving, transaction)?;
    }
    Ok(Book { accounts: states })
}

/// Lots that left an account and wait for their transfer in.
struct Moving {
    /// The day they left.
    date: Date,
    /// The account they left.
    from: i64,
    /// The lots, in their original order.
    lots: VecDeque<Lot>,
}

fn apply(
    states: &mut BTreeMap<i64, AccountState>,
    moving: &mut BTreeMap<i64, Vec<Moving>>,
    transaction: &Transaction,
) -> Result<(), BookError> {
    let Some(account) = states.get(&transaction.account_id) else {
        return Err(BookError::UnknownAccount {
            transaction: transaction.id,
            account: transaction.account_id,
        });
    };
    check_instrument(transaction)?;
    if transaction.kind.has_instrument() && !account.kind.holds_securities() {
        return Err(invalid(
            transaction,
            &format!(
                "a {} account does not hold securities",
                account.kind.as_str()
            ),
        ));
    }
    if transaction.fee < Decimal::ZERO {
        return Err(invalid(transaction, "a fee cannot be negative"));
    }
    if transaction.fee > Decimal::ZERO
        && matches!(
            transaction.kind,
            Kind::Split | Kind::TransferIn | Kind::TransferOut
        )
    {
        return Err(invalid(
            transaction,
            &format!("a {} does not carry a fee", transaction.kind.as_str()),
        ));
    }
    let rate = rate_of(account, transaction)?;
    let cash = cash_delta(transaction, rate)?;
    match transaction.kind {
        Kind::Buy => buy(states, transaction, rate)?,
        Kind::Sell => sell(states, transaction, rate)?,
        Kind::Split => split(states, transaction)?,
        Kind::TransferOut => transfer_out(states, moving, transaction)?,
        Kind::TransferIn => transfer_in(states, moving, transaction, rate)?,
        Kind::Dividend | Kind::Fee | Kind::Deposit | Kind::Withdrawal | Kind::Interest => {}
    }
    let account = states
        .get_mut(&transaction.account_id)
        .expect("the account was found");
    account.cash = add(account.cash, cash, transaction)?;
    Ok(())
}

fn check_instrument(transaction: &Transaction) -> Result<(), BookError> {
    match (transaction.kind.has_instrument(), transaction.instrument_id) {
        (true, None) => Err(invalid(
            transaction,
            &format!("a {} needs an instrument", transaction.kind.as_str()),
        )),
        (false, Some(_)) => Err(invalid(
            transaction,
            &format!(
                "a {} does not carry an instrument",
                transaction.kind.as_str()
            ),
        )),
        _ => Ok(()),
    }
}

/// Books new units into a lot whose cost includes the fee.
fn buy(
    states: &mut BTreeMap<i64, AccountState>,
    transaction: &Transaction,
    rate: Decimal,
) -> Result<(), BookError> {
    let quantity = required(transaction, transaction.quantity, "quantity")?;
    let price = required(transaction, transaction.price, "price")?;
    let fee = div(transaction.fee, quantity, transaction)?;
    let lot = Lot {
        acquired: transaction.date,
        quantity,
        unit_cost: mul(add(price, fee, transaction)?, rate, transaction)?,
    };
    let holding = holding_mut(states, transaction)?;
    holding.lots.push(lot);
    refresh(holding, transaction)?;
    Ok(())
}

/// Consumes the oldest units and books the result against their own cost.
fn sell(
    states: &mut BTreeMap<i64, AccountState>,
    transaction: &Transaction,
    rate: Decimal,
) -> Result<(), BookError> {
    let quantity = required(transaction, transaction.quantity, "quantity")?;
    let price = required(transaction, transaction.price, "price")?;
    let fee = div(transaction.fee, quantity, transaction)?;
    let net_unit_price = mul(sub(price, fee, transaction)?, rate, transaction)?;
    let instrument = instrument(transaction)?;
    let account = states
        .get_mut(&transaction.account_id)
        .expect("the account was found");
    let taken = take_from(account, instrument, quantity, transaction)?;
    let mut gain = Decimal::ZERO;
    for lot in &taken {
        let margin = sub(net_unit_price, lot.unit_cost, transaction)?;
        gain = add(gain, mul(margin, lot.quantity, transaction)?, transaction)?;
    }
    account.realized = add(account.realized, gain, transaction)?;
    Ok(())
}

/// Rescales the units and their unit cost so the holding's total cost stays.
fn split(
    states: &mut BTreeMap<i64, AccountState>,
    transaction: &Transaction,
) -> Result<(), BookError> {
    let ratio = required(transaction, transaction.quantity, "ratio")?;
    let instrument = instrument(transaction)?;
    let account = states
        .get_mut(&transaction.account_id)
        .expect("the account was found");
    if let Some(holding) = account.holdings.get_mut(&instrument) {
        for lot in &mut holding.lots {
            lot.quantity = mul(lot.quantity, ratio, transaction)?;
            lot.unit_cost = div(lot.unit_cost, ratio, transaction)?;
        }
        refresh(holding, transaction)?;
    }
    Ok(())
}

fn transfer_out(
    states: &mut BTreeMap<i64, AccountState>,
    moving: &mut BTreeMap<i64, Vec<Moving>>,
    transaction: &Transaction,
) -> Result<(), BookError> {
    let quantity = required(transaction, transaction.quantity, "quantity")?;
    let instrument = instrument(transaction)?;
    let account = states
        .get_mut(&transaction.account_id)
        .expect("the account was found");
    let taken = take_from(account, instrument, quantity, transaction)?;
    moving.entry(instrument).or_default().push(Moving {
        date: transaction.date,
        from: transaction.account_id,
        lots: taken.into(),
    });
    Ok(())
}

fn transfer_in(
    states: &mut BTreeMap<i64, AccountState>,
    moving: &mut BTreeMap<i64, Vec<Moving>>,
    transaction: &Transaction,
    rate: Decimal,
) -> Result<(), BookError> {
    let quantity = required(transaction, transaction.quantity, "quantity")?;
    let instrument = instrument(transaction)?;
    let lots = {
        let moved = moving.entry(instrument).or_default();
        let mut paired = None;
        for (index, entry) in moved.iter().enumerate() {
            if entry.date == transaction.date && total_of(entry, transaction)? == quantity {
                paired = Some(index);
                break;
            }
        }
        if let Some(index) = paired {
            let entry = moved.remove(index);
            let source = states.get(&entry.from).expect("the account was found");
            let receiver = states
                .get(&transaction.account_id)
                .expect("the account was found");
            if source.currency != receiver.currency {
                return Err(BookError::TransferCurrency {
                    transaction: transaction.id,
                });
            }
            entry.lots.into_iter().collect()
        } else {
            let price = required(transaction, transaction.price, "price")?;
            vec![Lot {
                acquired: transaction.date,
                quantity,
                unit_cost: mul(price, rate, transaction)?,
            }]
        }
    };
    let holding = holding_mut(states, transaction)?;
    holding.lots.extend(lots);
    holding.lots.sort_by_key(|lot| lot.acquired);
    refresh(holding, transaction)?;
    Ok(())
}

/// The units a pending transfer carries.
fn total_of(entry: &Moving, transaction: &Transaction) -> Result<Decimal, BookError> {
    entry.lots.iter().try_fold(Decimal::ZERO, |total, lot| {
        add(total, lot.quantity, transaction)
    })
}

/// The holding `transaction` names, created empty when the account has none.
fn holding_mut<'a>(
    states: &'a mut BTreeMap<i64, AccountState>,
    transaction: &Transaction,
) -> Result<&'a mut Holding, BookError> {
    let instrument = instrument(transaction)?;
    Ok(states
        .get_mut(&transaction.account_id)
        .expect("the account was found")
        .holdings
        .entry(instrument)
        .or_insert_with(empty_holding))
}

/// Recomputes the totals the lots imply.
fn refresh(holding: &mut Holding, transaction: &Transaction) -> Result<(), BookError> {
    let mut quantity = Decimal::ZERO;
    let mut basis = Decimal::ZERO;
    for lot in &holding.lots {
        quantity = add(quantity, lot.quantity, transaction)?;
        basis = add(
            basis,
            mul(lot.quantity, lot.unit_cost, transaction)?,
            transaction,
        )?;
    }
    holding.quantity = quantity;
    holding.cost_basis = basis;
    Ok(())
}

fn empty_holding() -> Holding {
    Holding {
        lots: Vec::new(),
        quantity: Decimal::ZERO,
        cost_basis: Decimal::ZERO,
    }
}

/// Takes `quantity` units out of `account`'s holding of `instrument`, dropping
/// the holding once its last lot is gone.
fn take_from(
    account: &mut AccountState,
    instrument: i64,
    quantity: Decimal,
    transaction: &Transaction,
) -> Result<Vec<Lot>, BookError> {
    let holding = account
        .holdings
        .entry(instrument)
        .or_insert_with(empty_holding);
    let taken = take_units(holding, quantity, transaction)?;
    if holding.lots.is_empty() {
        account.holdings.remove(&instrument);
    }
    Ok(taken)
}

/// Removes `quantity` units from `holding`, oldest lots first, and returns the
/// lots taken.
fn take_units(
    holding: &mut Holding,
    quantity: Decimal,
    transaction: &Transaction,
) -> Result<Vec<Lot>, BookError> {
    let held = holding.lots.iter().try_fold(Decimal::ZERO, |total, lot| {
        add(total, lot.quantity, transaction)
    })?;
    if quantity > held {
        return Err(BookError::Oversold {
            transaction: transaction.id,
            instrument: transaction.instrument_id.unwrap_or_default(),
            wanted: quantity,
            held,
        });
    }
    let mut left = quantity;
    let mut taken = Vec::new();
    while left > Decimal::ZERO {
        let lot = holding.lots.first_mut().expect("enough units are held");
        let take = lot.quantity.min(left);
        taken.push(Lot {
            quantity: take,
            ..lot.clone()
        });
        lot.quantity = sub(lot.quantity, take, transaction)?;
        left = sub(left, take, transaction)?;
        if lot.quantity == Decimal::ZERO {
            holding.lots.remove(0);
        }
    }
    refresh(holding, transaction)?;
    Ok(taken)
}

/// The cash change of `transaction` in the account's currency. Split and transfer
/// move units, not money.
fn cash_delta(transaction: &Transaction, rate: Decimal) -> Result<Decimal, BookError> {
    let fee = transaction.fee;
    let delta = match transaction.kind {
        Kind::Buy => {
            let quantity = required(transaction, transaction.quantity, "quantity")?;
            let price = required(transaction, transaction.price, "price")?;
            let cost = add(mul(quantity, price, transaction)?, fee, transaction)?;
            negate(mul(cost, rate, transaction)?, transaction)?
        }
        Kind::Sell => {
            let quantity = required(transaction, transaction.quantity, "quantity")?;
            let price = required(transaction, transaction.price, "price")?;
            let proceeds = sub(mul(quantity, price, transaction)?, fee, transaction)?;
            mul(proceeds, rate, transaction)?
        }
        Kind::Dividend | Kind::Deposit | Kind::Interest => {
            let amount = required(transaction, transaction.amount, "amount")?;
            mul(sub(amount, fee, transaction)?, rate, transaction)?
        }
        Kind::Fee | Kind::Withdrawal => {
            let amount = required(transaction, transaction.amount, "amount")?;
            negate(
                mul(add(amount, fee, transaction)?, rate, transaction)?,
                transaction,
            )?
        }
        Kind::Split | Kind::TransferIn | Kind::TransferOut => Decimal::ZERO,
    };
    Ok(delta)
}

/// The value of `left` times `right`, refused when it does not fit.
fn mul(left: Decimal, right: Decimal, transaction: &Transaction) -> Result<Decimal, BookError> {
    left.checked_mul(right)
        .ok_or_else(|| out_of_range(transaction))
}

/// The value of `left` plus `right`, refused when it does not fit.
fn add(left: Decimal, right: Decimal, transaction: &Transaction) -> Result<Decimal, BookError> {
    left.checked_add(right)
        .ok_or_else(|| out_of_range(transaction))
}

/// The value of `left` minus `right`, refused when it does not fit.
fn sub(left: Decimal, right: Decimal, transaction: &Transaction) -> Result<Decimal, BookError> {
    left.checked_sub(right)
        .ok_or_else(|| out_of_range(transaction))
}

/// The value of `left` divided by `right`, refused when it does not fit.
fn div(left: Decimal, right: Decimal, transaction: &Transaction) -> Result<Decimal, BookError> {
    left.checked_div(right)
        .ok_or_else(|| out_of_range(transaction))
}

/// The value of `-value`, refused when it does not fit.
fn negate(value: Decimal, transaction: &Transaction) -> Result<Decimal, BookError> {
    Decimal::ZERO
        .checked_sub(value)
        .ok_or_else(|| out_of_range(transaction))
}

/// An amount or a quantity the engine cannot carry.
fn out_of_range(transaction: &Transaction) -> BookError {
    invalid(transaction, "an amount is out of range")
}

/// The account currency per transaction currency.
fn rate_of(account: &AccountState, transaction: &Transaction) -> Result<Decimal, BookError> {
    if account.currency == transaction.currency {
        return Ok(Decimal::ONE);
    }
    let rate = transaction.fx_rate.ok_or_else(|| {
        invalid(
            transaction,
            "a transaction in another currency needs fx_rate",
        )
    })?;
    if rate <= Decimal::ZERO {
        return Err(invalid(transaction, "fx_rate must be positive"));
    }
    Ok(rate)
}

/// The fee of one unit, spread evenly.
/// The instrument the transaction names, which its kind requires.
fn instrument(transaction: &Transaction) -> Result<i64, BookError> {
    transaction.instrument_id.ok_or_else(|| {
        invalid(
            transaction,
            &format!("a {} needs an instrument", transaction.kind.as_str()),
        )
    })
}

fn required(
    transaction: &Transaction,
    value: Option<Decimal>,
    name: &str,
) -> Result<Decimal, BookError> {
    match value {
        Some(value) if value > Decimal::ZERO => Ok(value),
        _ => Err(invalid(
            transaction,
            &format!("a {} needs a positive {name}", transaction.kind.as_str()),
        )),
    }
}

fn invalid(transaction: &Transaction, reason: &str) -> BookError {
    BookError::Invalid {
        transaction: transaction.id,
        reason: reason.to_owned(),
    }
}
