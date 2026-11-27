//! Portfolio performance over a date range: a daily value series, a time-weighted
//! return, a money-weighted return (XIRR), a benchmark, and allocation.
//!
//! Values are worked out in the portfolio's base currency. A holding is valued at
//! its newest price on or before the day, carried forward; cash is valued at face
//! in its account's currency. Rates are ECB units per euro, so a value in another
//! currency goes through the euro.
//!
//! Conventions, settled with the owner:
//! - An external flow (a deposit or a withdrawal) lands at the end of its day, so
//!   the day's time-weighted return is `(V_end - flow) / V_start - 1`.
//! - XIRR uses actual days over 365 and the window's opening value as its first
//!   flow, so it measures the requested range.
//! - A day whose starting value is zero has no return and is skipped.
//! - A holding or a currency with no price or rate counts as zero and is named in
//!   the report, so a gap is visible rather than invented.

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::Date;
use rust_decimal::Decimal;

use crate::book::{self, Account, Book, BookError, Kind, Transaction};

/// The asset classes an instrument can carry.
pub const ASSET_CLASSES: [&str; 7] = [
    "equity",
    "bond",
    "cash",
    "real_estate",
    "commodity",
    "crypto",
    "other",
];

/// The regions an instrument can carry.
pub const REGIONS: [&str; 6] = [
    "europe",
    "north_america",
    "asia",
    "emerging",
    "global",
    "other",
];

/// A dated close or net asset value, in the instrument's own currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Close {
    /// The trading day.
    pub date: Date,
    /// The close or NAV.
    pub value: Decimal,
}

/// What the engine needs to value a portfolio.
#[derive(Debug, Clone, Default)]
pub struct Market {
    /// Closes by instrument, oldest first.
    pub prices: BTreeMap<i64, Vec<Close>>,
    /// The currency each held instrument is priced in.
    pub currencies: BTreeMap<i64, String>,
    /// ECB days, oldest first: units of each currency per euro.
    pub fx: Vec<(Date, BTreeMap<String, Decimal>)>,
}

/// How an instrument is classified for allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentInfo {
    /// One of [`ASSET_CLASSES`].
    pub asset_class: String,
    /// One of [`REGIONS`].
    pub region: String,
}

/// Target weights per dimension, in percent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    /// By asset class.
    pub asset_class: BTreeMap<String, Decimal>,
    /// By region.
    pub region: BTreeMap<String, Decimal>,
}

/// Everything the engine reads.
#[derive(Debug)]
pub struct Input<'a> {
    /// The portfolio's accounts.
    pub accounts: &'a [Account],
    /// Its transactions.
    pub transactions: &'a [Transaction],
    /// The ISO 4217 currency values are reported in.
    pub base_currency: &'a str,
    /// Prices and rates.
    pub market: &'a Market,
    /// Classification by instrument.
    pub instruments: &'a BTreeMap<i64, InstrumentInfo>,
    /// The benchmark instrument, when one is configured and priced.
    pub benchmark: Option<i64>,
    /// The target weights, when the portfolio has them.
    pub targets: Option<&'a Targets>,
    /// The first day of the range.
    pub from: Date,
    /// The last day of the range.
    pub to: Date,
}

/// One day of the value series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// The day.
    pub date: Date,
    /// The portfolio's value at its close.
    pub value: Decimal,
    /// The net external flow on that day, positive for a deposit.
    pub flow: Decimal,
}

/// One day of the benchmark series, normalized to 100 at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkPoint {
    /// The day.
    pub date: Date,
    /// The benchmark index, 100 at the range's start.
    pub value: Decimal,
}

/// One dimension of the allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// The asset class or region.
    pub key: String,
    /// Its market value in the base currency.
    pub value: Decimal,
    /// Its share of the portfolio, in percent.
    pub weight_percent: Decimal,
    /// The target share, when a target is set.
    pub target_percent: Option<Decimal>,
    /// `weight_percent - target_percent`, in percentage points.
    pub drift_pp: Option<Decimal>,
}

/// The allocation at the end of the range.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allocation {
    /// By asset class.
    pub asset_class: Vec<Share>,
    /// By region.
    pub region: Vec<Share>,
}

/// The engine's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The daily values.
    pub points: Vec<Point>,
    /// The time-weighted return, in percent.
    pub twr_percent: Option<Decimal>,
    /// The money-weighted return (XIRR), in percent.
    pub xirr_percent: Option<Decimal>,
    /// The benchmark's return over the range, in percent.
    pub benchmark_percent: Option<Decimal>,
    /// The benchmark index, 100 at the range's start.
    pub benchmark: Vec<BenchmarkPoint>,
    /// The allocation at the end of the range.
    pub allocation: Allocation,
    /// Instruments that had no price at some point and counted as zero.
    pub unpriced_instruments: Vec<i64>,
    /// Currencies that had no rate and counted as zero.
    pub unpriced_currencies: Vec<String>,
}

/// Reads the performance report of `input`.
pub fn read(input: &Input<'_>) -> Result<Report, BookError> {
    if input.to < input.from {
        return Ok(Report {
            points: Vec::new(),
            twr_percent: None,
            xirr_percent: None,
            benchmark_percent: None,
            benchmark: Vec::new(),
            allocation: Allocation::default(),
            unpriced_instruments: Vec::new(),
            unpriced_currencies: Vec::new(),
        });
    }
    let mut gaps = Gaps::default();
    let mut points = Vec::new();
    let mut day = input.from;
    loop {
        let book = book::read_until(input.accounts, input.transactions, day)?;
        let value = value_of(&book, input, day, &mut gaps);
        let flow = flow_of(input, day, &mut gaps);
        points.push(Point {
            date: day,
            value,
            flow,
        });
        if day == input.to {
            break;
        }
        day = day.tomorrow().map_err(|_| BookError::Invalid {
            transaction: 0,
            reason: format!("the range cannot pass {day}"),
        })?;
    }
    let book = book::read_until(input.accounts, input.transactions, input.to)?;
    let (benchmark, benchmark_percent) = benchmark_of(input, &mut gaps);
    Ok(Report {
        twr_percent: twr(&points),
        xirr_percent: xirr(input, &points, &mut gaps),
        points,
        benchmark_percent,
        benchmark,
        allocation: allocation_of(&book, input, &mut gaps),
        unpriced_instruments: gaps.instruments.into_iter().collect(),
        unpriced_currencies: gaps.currencies.into_iter().collect(),
    })
}

/// Where valuation had to use zero.
#[derive(Debug, Default)]
struct Gaps {
    instruments: BTreeSet<i64>,
    currencies: BTreeSet<String>,
}

/// The value of `book` at the close of `day`, in the base currency.
fn value_of(book: &Book, input: &Input<'_>, day: Date, gaps: &mut Gaps) -> Decimal {
    let mut total = Decimal::ZERO;
    for state in book.accounts.values() {
        if !state.cash.is_zero() {
            match to_base(
                input.market,
                state.cash,
                &state.currency,
                input.base_currency,
                day,
            ) {
                Some(cash) => total += cash,
                None => {
                    gaps.currencies.insert(state.currency.clone());
                }
            }
        }
        for (instrument, holding) in &state.holdings {
            let Some(close) = close_on(input.market, *instrument, day) else {
                gaps.instruments.insert(*instrument);
                continue;
            };
            let Some(currency) = input.market.currencies.get(instrument) else {
                gaps.instruments.insert(*instrument);
                continue;
            };
            let value = holding.quantity * close;
            match to_base(input.market, value, currency, input.base_currency, day) {
                Some(value) => total += value,
                None => {
                    gaps.currencies.insert(currency.clone());
                }
            }
        }
    }
    total
}

/// The net external flow of `day`, positive for money arriving.
fn flow_of(input: &Input<'_>, day: Date, gaps: &mut Gaps) -> Decimal {
    let mut flow = Decimal::ZERO;
    for transaction in input.transactions {
        if transaction.date != day || !matches!(transaction.kind, Kind::Deposit | Kind::Withdrawal)
        {
            continue;
        }
        let Some(account) = account_of(input, transaction.account_id) else {
            continue;
        };
        let Some(amount) = cash_moved(transaction, account, gaps) else {
            continue;
        };
        let amount = if transaction.kind == Kind::Deposit {
            amount
        } else {
            -amount
        };
        match to_base(
            input.market,
            amount,
            &account.currency,
            input.base_currency,
            day,
        ) {
            Some(value) => flow += value,
            None => {
                gaps.currencies.insert(account.currency.clone());
            }
        }
    }
    flow
}

/// The account a transaction belongs to.
fn account_of<'i>(input: &'i Input<'_>, id: i64) -> Option<&'i Account> {
    input.accounts.iter().find(|account| account.id == id)
}

/// The cash a deposit or withdrawal moves in its account's currency, as the book
/// records it: fees included, and a foreign amount converted at its `fx_rate`. A
/// fee is therefore part of the period's return, not external money.
fn cash_moved(transaction: &Transaction, account: &Account, gaps: &mut Gaps) -> Option<Decimal> {
    let amount = transaction.amount?;
    let net = match transaction.kind {
        Kind::Deposit => amount.checked_sub(transaction.fee)?,
        Kind::Withdrawal => amount.checked_add(transaction.fee)?,
        _ => return None,
    };
    if transaction.currency == account.currency {
        return Some(net);
    }
    match transaction.fx_rate {
        Some(rate) if rate > Decimal::ZERO => net.checked_mul(rate),
        _ => {
            gaps.currencies.insert(transaction.currency.clone());
            None
        }
    }
}

/// Units of `currency` per euro; the euro is the table's own unit.
fn per_eur(day: &BTreeMap<String, Decimal>, currency: &str) -> Option<Decimal> {
    if currency == "EUR" {
        Some(Decimal::ONE)
    } else {
        day.get(currency).copied()
    }
}

/// The newest close of `instrument` on or before `day`.
fn close_on(market: &Market, instrument: i64, day: Date) -> Option<Decimal> {
    let closes = market.prices.get(&instrument)?;
    let index = closes.partition_point(|close| close.date <= day);
    (index > 0).then_some(closes[index - 1].value)
}

/// The ECB day on or before `day`.
fn fx_on(market: &Market, day: Date) -> Option<&BTreeMap<String, Decimal>> {
    let index = market.fx.partition_point(|(date, _)| *date <= day);
    (index > 0).then(|| &market.fx[index - 1].1)
}

/// `amount` in `currency` valued in `base` on `day`.
fn to_base(
    market: &Market,
    amount: Decimal,
    currency: &str,
    base: &str,
    day: Date,
) -> Option<Decimal> {
    if currency == base {
        return Some(amount);
    }
    let day_rates = fx_on(market, day)?;
    let rate = per_eur(day_rates, currency)?;
    let base_rate = per_eur(day_rates, base)?;
    amount.checked_div(rate)?.checked_mul(base_rate)
}

/// The time-weighted return in percent, chaining each day's return.
fn twr(points: &[Point]) -> Option<Decimal> {
    if points.len() < 2 {
        return None;
    }
    let mut growth = 1.0f64;
    for pair in points.windows(2) {
        let (start, end) = (as_f64(pair[0].value), as_f64(pair[1].value));
        if start > 0.0 {
            growth *= (end - as_f64(pair[1].flow)) / start;
        }
    }
    percent(growth - 1.0)
}

/// The money-weighted return in percent, solved by scanning for a sign change and
/// bisecting.
fn xirr(input: &Input<'_>, points: &[Point], gaps: &mut Gaps) -> Option<Decimal> {
    let start = points.first()?.value;
    let end = points.last()?.value;
    let mut flows: Vec<(i64, f64)> = Vec::new();
    if start > Decimal::ZERO {
        flows.push((0, -as_f64(start)));
    }
    for transaction in input.transactions {
        if transaction.date <= input.from || transaction.date > input.to {
            continue;
        }
        if !matches!(transaction.kind, Kind::Deposit | Kind::Withdrawal) {
            continue;
        }
        let Some(account) = account_of(input, transaction.account_id) else {
            continue;
        };
        let Some(amount) = cash_moved(transaction, account, gaps) else {
            continue;
        };
        let amount = if transaction.kind == Kind::Deposit {
            -amount
        } else {
            amount
        };
        let Some(value) = to_base(
            input.market,
            amount,
            &account.currency,
            input.base_currency,
            transaction.date,
        ) else {
            gaps.currencies.insert(account.currency.clone());
            continue;
        };
        let days = input
            .from
            .until(transaction.date)
            .map_or(0, |span| i64::from(span.get_days()));
        flows.push((days, as_f64(value)));
    }
    if end > Decimal::ZERO {
        let days = input
            .from
            .until(input.to)
            .map_or(0, |span| i64::from(span.get_days()));
        flows.push((days, as_f64(end)));
    }
    if flows.len() < 2 || !has_both_signs(&flows) {
        return None;
    }
    let root = solve(&flows)?;
    percent(root)
}

/// Whether the flows have at least one of each sign.
fn has_both_signs(flows: &[(i64, f64)]) -> bool {
    flows.iter().any(|(_, flow)| *flow < 0.0) && flows.iter().any(|(_, flow)| *flow > 0.0)
}

/// Finds a rate where the net present value of `flows` is zero. The scan is dense
/// near zero and moves on a widening grid for the very high rates a short range
/// with an ordinary gain implies.
fn solve(flows: &[(i64, f64)]) -> Option<f64> {
    let mut low = None;
    let mut high = None;
    let mut previous: Option<(f64, f64)> = None;
    let rates = (-200..=1000).map(|step| f64::from(step) / 100.0).chain([
        20.0,
        50.0,
        100.0,
        1_000.0,
        10_000.0,
        100_000.0,
        1_000_000.0,
    ]);
    for rate in rates {
        let value = npv(flows, rate);
        if !value.is_finite() {
            continue;
        }
        if value == 0.0 {
            return Some(rate);
        }
        if let Some((last_rate, last_value)) = previous
            && last_value.signum() != value.signum()
        {
            low = Some(last_rate);
            high = Some(rate);
            break;
        }
        previous = Some((rate, value));
    }
    let (mut low, mut high) = (low?, high?);
    for _ in 0..200 {
        let mid = f64::midpoint(low, high);
        let value = npv(flows, mid);
        let low_value = npv(flows, low);
        if value.signum() == low_value.signum() {
            low = mid;
        } else {
            high = mid;
        }
    }
    Some(f64::midpoint(low, high))
}

/// Net present value of `flows`, discounted from `day` days after the start.
#[expect(
    clippy::cast_precision_loss,
    reason = "a day count is far below 2^53, so f64 holds it exactly"
)]
fn npv(flows: &[(i64, f64)], rate: f64) -> f64 {
    flows
        .iter()
        .map(|(days, flow)| {
            let exponent = (*days as f64) / 365.0;
            flow / (1.0 + rate).powf(exponent)
        })
        .sum()
}

/// The benchmark series, normalized to 100 at the range's start.
fn benchmark_of(input: &Input<'_>, gaps: &mut Gaps) -> (Vec<BenchmarkPoint>, Option<Decimal>) {
    let Some(instrument) = input.benchmark else {
        return (Vec::new(), None);
    };
    let Some(currency) = input.market.currencies.get(&instrument) else {
        // A benchmark without a price is not a holding gap; the series is simply
        // absent, and the report already says so by leaving the benchmark empty.
        return (Vec::new(), None);
    };
    let Some(first) = close_on(input.market, instrument, input.from) else {
        return (Vec::new(), None);
    };
    let Some(first) = to_base(
        input.market,
        first,
        currency,
        input.base_currency,
        input.from,
    ) else {
        gaps.currencies.insert(currency.clone());
        return (Vec::new(), None);
    };
    if first <= Decimal::ZERO {
        return (Vec::new(), None);
    }
    let mut points = Vec::new();
    let mut day = input.from;
    loop {
        if let Some(close) = close_on(input.market, instrument, day)
            && let Some(value) = to_base(input.market, close, currency, input.base_currency, day)
        {
            points.push(BenchmarkPoint {
                date: day,
                value: (value / first * Decimal::from(100)).round_dp(4),
            });
        }
        if day == input.to {
            break;
        }
        let Ok(next) = day.tomorrow() else {
            break;
        };
        day = next;
    }
    let percent = points
        .last()
        .map(|point| (point.value - Decimal::from(100)).round_dp(4));
    (points, percent)
}

/// The allocation at the close of the range, by asset class and region.
fn allocation_of(book: &Book, input: &Input<'_>, gaps: &mut Gaps) -> Allocation {
    let mut classes: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut regions: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut total = Decimal::ZERO;
    for state in book.accounts.values() {
        if !state.cash.is_zero() {
            match to_base(
                input.market,
                state.cash,
                &state.currency,
                input.base_currency,
                input.to,
            ) {
                Some(cash) => {
                    *classes.entry("cash".to_owned()).or_default() += cash;
                    *regions.entry("global".to_owned()).or_default() += cash;
                    total += cash;
                }
                None => {
                    gaps.currencies.insert(state.currency.clone());
                }
            }
        }
        for (instrument, holding) in &state.holdings {
            let value = close_on(input.market, *instrument, input.to)
                .zip(input.market.currencies.get(instrument))
                .and_then(|(close, currency)| {
                    to_base(
                        input.market,
                        holding.quantity * close,
                        currency,
                        input.base_currency,
                        input.to,
                    )
                });
            let Some(value) = value else {
                gaps.instruments.insert(*instrument);
                continue;
            };
            let info = input.instruments.get(instrument);
            let class = info.map_or("other", |info| info.asset_class.as_str());
            let region = info.map_or("other", |info| info.region.as_str());
            *classes.entry(class.to_owned()).or_default() += value;
            *regions.entry(region.to_owned()).or_default() += value;
            total += value;
        }
    }
    let targets = input.targets;
    Allocation {
        asset_class: shares(classes, total, targets.map(|targets| &targets.asset_class)),
        region: shares(regions, total, targets.map(|targets| &targets.region)),
    }
}

/// Turns values into weighted shares with their targets. A target with no value
/// is still shown, at zero, so its drift stays visible.
fn shares(
    mut values: BTreeMap<String, Decimal>,
    total: Decimal,
    targets: Option<&BTreeMap<String, Decimal>>,
) -> Vec<Share> {
    if let Some(targets) = targets {
        for key in targets.keys() {
            values.entry(key.clone()).or_insert(Decimal::ZERO);
        }
    }
    let mut shares: Vec<Share> = values
        .iter()
        .map(|(key, value)| {
            let weight = if total > Decimal::ZERO {
                (*value / total * Decimal::from(100)).round_dp(4)
            } else {
                Decimal::ZERO
            };
            let target = targets.and_then(|targets| targets.get(key).copied());
            Share {
                key: key.clone(),
                value: *value,
                weight_percent: weight,
                target_percent: target,
                drift_pp: target.map(|target| (weight - target).round_dp(4)),
            }
        })
        .collect();
    shares.sort_by(|left, right| {
        right
            .value
            .cmp(&left.value)
            .then_with(|| left.key.cmp(&right.key))
    });
    shares
}

/// A decimal as `f64`, which is enough for a rate reported to two decimals.
fn as_f64(value: Decimal) -> f64 {
    value.to_string().parse().unwrap_or_default()
}

/// `ratio` as a percentage, when it is a finite number.
fn percent(ratio: f64) -> Option<Decimal> {
    if !ratio.is_finite() {
        return None;
    }
    Decimal::from_f64_retain(ratio * 100.0).map(|value| value.round_dp(4))
}
