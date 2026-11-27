//! The portfolio book over HTTP: accounts, their transactions, and the holdings
//! and cash the engine computes from them.
//!
//! Every route lives under `/api/portfolios/{portfolio_id}` and behind the access
//! guard, so a caller only ever reads or changes their own portfolio.

pub mod accounts;
pub mod holdings;
pub mod transactions;

use saffron_core::book as engine;
use saffron_db::{accounts as db_accounts, transactions as db_transactions};

/// The accounts the engine reads, reduced to what it needs.
pub(crate) fn engine_accounts(accounts: &[db_accounts::Account]) -> Vec<engine::Account> {
    accounts
        .iter()
        .map(|account| engine::Account {
            id: account.id,
            kind: account.kind,
            currency: account.currency.clone(),
        })
        .collect()
}

/// The stored transactions in the fields the engine reads.
pub(crate) fn engine_transactions(
    transactions: &[db_transactions::Transaction],
) -> Vec<engine::Transaction> {
    transactions
        .iter()
        .map(|transaction| engine::Transaction {
            id: transaction.id,
            account_id: transaction.account_id,
            instrument_id: transaction.instrument_id,
            kind: transaction.kind,
            date: transaction.date,
            quantity: transaction.quantity,
            price: transaction.price,
            amount: transaction.amount,
            fee: transaction.fee,
            currency: transaction.currency.clone(),
            fx_rate: transaction.fx_rate,
        })
        .collect()
}
