//! Portfolio performance over HTTP: the daily value series, TWR, XIRR, the
//! benchmark, and allocation.

use std::collections::{BTreeMap, BTreeSet};

use axum::{Json, extract::State};
use jiff::civil::Date;
use saffron_core::{instrument::InstrumentKind, performance};
use saffron_db::{accounts, decimal, fx, instruments, prices, targets, transactions};
use serde::{Deserialize, Serialize};

use crate::{
    book::{engine_accounts, engine_transactions},
    error::{ApiError, ApiResult},
    portfolios::guard::PortfolioAccess,
    request::QueryParam,
    state::AppState,
};

/// The benchmark used when a portfolio names none: the iShares Core MSCI World
/// UCITS ETF, which an owner can add through the ordinary instrument lookup.
pub const DEFAULT_BENCHMARK_ISIN: &str = "IE00B4L5Y983";

/// The longest range the value series covers, in days: about twenty years. A
/// longer request is refused, and the default range starts no earlier than this.
pub const MAX_RANGE_DAYS: i64 = 20 * 366;

/// The asset class an instrument kind falls back to before an owner classifies it.
#[must_use]
pub const fn default_asset_class(kind: InstrumentKind) -> &'static str {
    match kind {
        InstrumentKind::Stock | InstrumentKind::Etf | InstrumentKind::Fund => "equity",
        InstrumentKind::Bond => "bond",
        InstrumentKind::Crypto => "crypto",
        InstrumentKind::Other => "other",
    }
}

/// The region a listing symbol's exchange suffix falls back to. This seed table
/// belongs to the server, not to the core domain, and a stored region always wins.
/// A dotted class-share suffix such as `BRK.B` is not an exchange and lands in
/// `other` until an owner classifies the instrument.
#[must_use]
pub fn default_region(symbol: Option<&str>) -> &'static str {
    let Some(symbol) = symbol else {
        return "other";
    };
    let suffix = symbol.rsplit_once('.').map(|(_, suffix)| suffix);
    match suffix {
        None | Some("TO" | "V" | "MX") => "north_america",
        Some(
            "HE" | "ST" | "OL" | "CO" | "L" | "DE" | "F" | "PA" | "AS" | "MI" | "MC" | "SW" | "VI"
            | "BR" | "LS" | "IR",
        ) => "europe",
        Some("T" | "HK" | "SI" | "KS" | "TW" | "AX" | "NZ") => "asia",
        Some("SA" | "JO") => "emerging",
        Some(_) => "other",
    }
}

/// Query of `GET /api/portfolios/{portfolio_id}/performance`.
#[derive(Debug, Deserialize)]
pub struct PerformanceQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

/// The performance of a portfolio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceView {
    /// The first day of the range.
    pub from: String,
    /// The last day of the range.
    pub to: String,
    /// The ISO 4217 currency values are in.
    pub base_currency: String,
    /// The daily value series.
    pub points: Vec<PointView>,
    /// The time-weighted return, in percent.
    pub twr_percent: Option<String>,
    /// The money-weighted return (XIRR), in percent.
    pub xirr_percent: Option<String>,
    /// The benchmark's return over the range, in percent.
    pub benchmark_percent: Option<String>,
    /// The benchmark series, 100 at the range's start.
    pub benchmark: Option<BenchmarkView>,
    /// The allocation at the end of the range.
    pub allocation: AllocationView,
    /// Instruments that had no price and counted as zero.
    pub unpriced_instruments: Vec<i64>,
    /// Currencies that had no rate and counted as zero.
    pub unpriced_currencies: Vec<String>,
}

/// One day of the value series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointView {
    /// The day.
    pub date: String,
    /// The portfolio's value at its close, as decimal text.
    pub value: String,
    /// The net external flow, positive for a deposit, as decimal text.
    pub flow: String,
}

/// The benchmark and its series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkView {
    /// The benchmark instrument.
    pub instrument_id: i64,
    /// The index, 100 at the range's start.
    pub points: Vec<BenchmarkPointView>,
}

/// One day of the benchmark series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkPointView {
    /// The day.
    pub date: String,
    /// The index value, as decimal text.
    pub value: String,
}

/// The allocation of the book.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationView {
    /// By asset class.
    pub asset_class: Vec<ShareView>,
    /// By region.
    pub region: Vec<ShareView>,
}

/// One dimension of the allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareView {
    /// The asset class or region.
    pub key: String,
    /// Its market value, as decimal text.
    pub value: String,
    /// Its share of the portfolio, in percent, as decimal text.
    pub weight_percent: String,
    /// The target share, when set, as decimal text.
    pub target_percent: Option<String>,
    /// The difference from the target, in percentage points.
    pub drift_pp: Option<String>,
}

/// `GET /api/portfolios/{portfolio_id}/performance[?from=…&to=…]`
pub async fn show(
    State(state): State<AppState>,
    access: PortfolioAccess,
    QueryParam(query): QueryParam<PerformanceQuery>,
) -> ApiResult<Json<PerformanceView>> {
    let portfolio = &access.0.portfolio;
    let today = state
        .clock
        .now()
        .to_zoned_datetime(state.timezone.clone())
        .date();
    let stored = transactions::list(&state.pool, portfolio.id).await?;
    let to = match query.to.as_deref() {
        Some(text) => parse_date(text)?,
        None => today,
    };
    let from = if let Some(text) = query.from.as_deref() {
        parse_date(text)?
    } else {
        let earliest = stored
            .iter()
            .map(|row| row.date)
            .min()
            .unwrap_or(today)
            .min(to);
        let floor = to
            .checked_sub(jiff::Span::new().days(MAX_RANGE_DAYS))
            .unwrap_or(earliest);
        earliest.max(floor)
    };
    if from > to {
        return Err(ApiError::bad_request(
            "invalid_range",
            "the start is after the end",
        ));
    }
    if i64::from(from.until(to).map_or(0, |span| span.get_days())) > MAX_RANGE_DAYS {
        return Err(ApiError::bad_request(
            "range_too_long",
            "the range covers more than twenty years; narrow it",
        ));
    }
    let (report, benchmark) = report_of(&state, portfolio, stored, from, to).await?;
    Ok(Json(view(
        &report,
        benchmark,
        from,
        to,
        &portfolio.base_currency,
    )))
}

/// Values `portfolio` over `from` to `to`, and reports which benchmark it used.
async fn report_of(
    state: &AppState,
    portfolio: &saffron_db::portfolios::Portfolio,
    stored: Vec<transactions::Transaction>,
    from: Date,
    to: Date,
) -> ApiResult<(performance::Report, Option<i64>)> {
    let accounts = accounts::list(&state.pool, portfolio.id).await?;
    let summaries = instruments::summaries(&state.pool).await?;
    let by_id: BTreeMap<i64, &instruments::Summary> = summaries
        .iter()
        .map(|summary| (summary.id, summary))
        .collect();
    let benchmark = benchmark_of(state, portfolio, &by_id).await?;
    let mut ids: BTreeSet<i64> = stored
        .iter()
        .filter_map(|row| row.instrument_id)
        .filter(|id| by_id.contains_key(id))
        .collect();
    if let Some(benchmark) = benchmark {
        ids.insert(benchmark);
    }
    let market = market_of(state, &ids, &by_id, from, to).await?;
    let infos = classifications(&summaries);
    let targets = targets_of(state, portfolio.id).await?;
    let base_currency = portfolio.base_currency.clone();
    // Rebuilding the book for every day is CPU work; keep it off the async worker.
    let report = tokio::task::spawn_blocking(move || {
        let core_accounts = engine_accounts(&accounts);
        let core_transactions = engine_transactions(&stored);
        performance::read(&performance::Input {
            accounts: &core_accounts,
            transactions: &core_transactions,
            base_currency: &base_currency,
            market: &market,
            instruments: &infos,
            benchmark,
            targets: targets.as_ref(),
            from,
            to,
        })
    })
    .await
    .map_err(|error| {
        ApiError::internal(format!(
            "the performance calculation did not finish: {error}"
        ))
    })?
    .map_err(|error| ApiError::internal(format!("the stored book is not valid: {error}")))?;
    Ok((report, benchmark))
}

/// The portfolio's chosen benchmark, or the default one when it has that instrument.
async fn benchmark_of(
    state: &AppState,
    portfolio: &saffron_db::portfolios::Portfolio,
    by_id: &BTreeMap<i64, &instruments::Summary>,
) -> ApiResult<Option<i64>> {
    match portfolio.benchmark_instrument_id {
        Some(id) if by_id.contains_key(&id) => Ok(Some(id)),
        Some(_) => Ok(None),
        None => Ok(instruments::id_by_isin(&state.pool, DEFAULT_BENCHMARK_ISIN)
            .await?
            .filter(|id| by_id.contains_key(id))),
    }
}

/// Prices up to `to` and ECB days from the last one before `from` through `to`.
async fn market_of(
    state: &AppState,
    ids: &BTreeSet<i64>,
    by_id: &BTreeMap<i64, &instruments::Summary>,
    from: Date,
    to: Date,
) -> ApiResult<performance::Market> {
    let mut market = performance::Market::default();
    let ids: Vec<i64> = ids.iter().copied().collect();
    for price in prices::until(&state.pool, &ids, to).await? {
        market
            .prices
            .entry(price.instrument_id)
            .or_default()
            .push(performance::Close {
                date: price.date,
                value: price.close,
            });
    }
    for id in ids {
        if let Some(summary) = by_id.get(&id) {
            market.currencies.insert(id, summary.currency.clone());
        }
    }
    market.fx = fx::between_with_prior(&state.pool, from, to).await?;
    Ok(market)
}

/// Every instrument's classification, stored or derived.
fn classifications(
    summaries: &[instruments::Summary],
) -> BTreeMap<i64, performance::InstrumentInfo> {
    summaries
        .iter()
        .map(|summary| {
            (
                summary.id,
                performance::InstrumentInfo {
                    asset_class: summary
                        .asset_class
                        .clone()
                        .unwrap_or_else(|| default_asset_class(summary.kind).to_owned()),
                    region: summary
                        .region
                        .clone()
                        .unwrap_or_else(|| default_region(summary.symbol.as_deref()).to_owned()),
                },
            )
        })
        .collect()
}

/// The stored targets, or `None` when the portfolio sets none.
async fn targets_of(
    state: &AppState,
    portfolio_id: i64,
) -> ApiResult<Option<performance::Targets>> {
    let stored = targets::get(&state.pool, portfolio_id).await?;
    if stored.asset_class.is_empty() && stored.region.is_empty() {
        return Ok(None);
    }
    Ok(Some(performance::Targets {
        asset_class: stored.asset_class,
        region: stored.region,
    }))
}

fn view(
    report: &performance::Report,
    benchmark: Option<i64>,
    from: Date,
    to: Date,
    base_currency: &str,
) -> PerformanceView {
    PerformanceView {
        from: from.to_string(),
        to: to.to_string(),
        base_currency: base_currency.to_owned(),
        points: report
            .points
            .iter()
            .map(|point| PointView {
                date: point.date.to_string(),
                value: decimal::to_text(point.value),
                flow: decimal::to_text(point.flow),
            })
            .collect(),
        twr_percent: report.twr_percent.map(decimal::to_text),
        xirr_percent: report.xirr_percent.map(decimal::to_text),
        benchmark_percent: report.benchmark_percent.map(decimal::to_text),
        benchmark: benchmark
            .filter(|_| !report.benchmark.is_empty())
            .map(|instrument_id| BenchmarkView {
                instrument_id,
                points: report
                    .benchmark
                    .iter()
                    .map(|point| BenchmarkPointView {
                        date: point.date.to_string(),
                        value: decimal::to_text(point.value),
                    })
                    .collect(),
            }),
        allocation: AllocationView {
            asset_class: shares(&report.allocation.asset_class),
            region: shares(&report.allocation.region),
        },
        unpriced_instruments: report.unpriced_instruments.clone(),
        unpriced_currencies: report.unpriced_currencies.clone(),
    }
}

fn shares(shares: &[performance::Share]) -> Vec<ShareView> {
    shares
        .iter()
        .map(|share| ShareView {
            key: share.key.clone(),
            value: decimal::to_text(share.value),
            weight_percent: decimal::to_text(share.weight_percent),
            target_percent: share.target_percent.map(decimal::to_text),
            drift_pp: share.drift_pp.map(decimal::to_text),
        })
        .collect()
}

fn parse_date(text: &str) -> ApiResult<Date> {
    text.trim()
        .parse()
        .map_err(|_| ApiError::bad_request("invalid_date", "a date is YYYY-MM-DD"))
}
