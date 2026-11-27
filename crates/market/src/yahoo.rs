//! Yahoo Finance: symbol search, listing details, batched quotes, and history.
//!
//! The endpoints are unofficial and need no account. Requests go through the
//! shared [`Client`], which sends a browser-like `User-Agent` and spaces them
//! [`LIVE_GAP`] apart.

use std::{str::FromStr, time::Duration};

use jiff::{Timestamp, civil::Date, civil::Time, tz::TimeZone};
use rust_decimal::Decimal;
use saffron_core::instrument::InstrumentKind;
use serde::Deserialize;

use crate::{DailyClose, History, Listing, MarketError, SearchHit, Spot, client::Client};

/// The name Saffron stores for this provider.
pub const PROVIDER: &str = "yahoo";
/// Where the live endpoints are.
pub const LIVE_URL: &str = "https://query2.finance.yahoo.com";
/// The shortest time between two requests to the live endpoints.
pub const LIVE_GAP: Duration = Duration::from_secs(1);
/// Most results one search returns.
pub const SEARCH_LIMIT: usize = 10;

/// A Yahoo Finance client.
#[derive(Debug)]
pub struct Yahoo {
    client: Client,
}

impl Yahoo {
    /// A client for the live endpoints.
    pub fn live() -> Result<Self, MarketError> {
        Self::new(LIVE_URL, LIVE_GAP)
    }

    /// A client for the endpoints at `base`, spacing requests `gap` apart.
    pub fn new(base: &str, gap: Duration) -> Result<Self, MarketError> {
        Ok(Self {
            client: Client::new(base, gap)?,
        })
    }

    /// Listings matching an ISIN, a symbol, or a name. Kinds Saffron cannot hold,
    /// such as indices and currencies, are left out.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>, MarketError> {
        let mut url = self.client.url(&["v1", "finance", "search"]);
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("quotesCount", &SEARCH_LIMIT.to_string())
            .append_pair("newsCount", "0")
            .append_pair("listsCount", "0")
            .append_pair("enableFuzzyQuery", "false");
        let found: SearchResponse = self.client.get_json(url).await?;
        Ok(found
            .quotes
            .into_iter()
            .filter_map(SearchQuote::hit)
            .collect())
    }

    /// The latest price of each of `symbols` in one request, quoted in the
    /// provider's currency (`GBp` stays pence; see [`crate::normalize`]). Symbols
    /// the provider has no price for are left out.
    pub async fn quotes(&self, symbols: &[&str]) -> Result<Vec<Spot>, MarketError> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }
        let mut url = self.client.url(&["v8", "finance", "spark"]);
        url.query_pairs_mut()
            .append_pair("symbols", &symbols.join(","))
            .append_pair("range", "1d")
            .append_pair("interval", "1d");
        let answer: serde_json::Value = self.client.get_json(url).await?;
        let spots = spark_spots(&answer)?;
        if spots.is_empty() {
            return Err(MarketError::BadResponse(
                "the quote answer holds no price series".to_owned(),
            ));
        }
        Ok(spots)
    }

    /// The daily closes of `symbol` from `from` to `to`, oldest first, valued in
    /// the major currency (see [`crate::normalize`]).
    pub async fn history(
        &self,
        symbol: &str,
        from: Date,
        to: Date,
    ) -> Result<History, MarketError> {
        let mut url = self.client.url(&["v8", "finance", "chart", symbol]);
        let midnight = |date: Date| {
            TimeZone::UTC
                .to_timestamp(date.to_datetime(Time::midnight()))
                .map(jiff::Timestamp::as_second)
        };
        let (Ok(from), Ok(to)) = (midnight(from), midnight(to)) else {
            return Err(MarketError::BadResponse("dates out of range".to_owned()));
        };
        url.query_pairs_mut()
            .append_pair("period1", &from.to_string())
            .append_pair("period2", &(to + 24 * 60 * 60).to_string())
            .append_pair("interval", "1d");
        let chart: ChartResponse = self.client.get_json(url).await?;
        let result = chart
            .chart
            .result
            .and_then(|results| results.into_iter().next())
            .ok_or(MarketError::NotFound)?;
        let quoted = result
            .meta
            .currency
            .clone()
            .ok_or_else(|| MarketError::BadResponse(format!("{symbol} has no currency")))?;
        let timezone = result
            .meta
            .exchange_timezone_name
            .as_deref()
            .and_then(|name| TimeZone::get(name).ok())
            .unwrap_or(TimeZone::UTC);
        let closes = result
            .indicators
            .quote
            .into_iter()
            .next()
            .unwrap_or_default();
        let mut history = History {
            symbol: result.meta.symbol,
            ..History::default()
        };
        for (timestamp, close) in result.timestamp.iter().zip(closes.close) {
            let Some(close) = decimal_of(&close) else {
                continue;
            };
            let date = Timestamp::from_second(*timestamp)
                .map_err(|error| MarketError::BadResponse(error.to_string()))?
                .to_zoned_datetime(timezone.clone())
                .date();
            let (currency, close) = crate::normalize(&quoted, close).ok_or_else(|| {
                MarketError::BadResponse(format!("{symbol} has currency {quoted:?}"))
            })?;
            history.currency = currency;
            history.closes.push(DailyClose { date, close });
        }
        if history.closes.is_empty() {
            return Err(MarketError::NotFound);
        }
        history.closes.sort_by_key(|close| close.date);
        history.closes.dedup_by_key(|close| close.date);
        Ok(history)
    }

    /// The listing `symbol`: its name, kind, currency, and exchange.
    pub async fn details(&self, symbol: &str) -> Result<Listing, MarketError> {
        let url = self.client.url(&["v8", "finance", "chart", symbol]);
        let chart: ChartResponse = self.client.get_json(url).await?;
        let meta = chart
            .chart
            .result
            .and_then(|results| results.into_iter().next())
            .ok_or(MarketError::NotFound)?
            .meta;
        meta.listing()
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    quotes: Vec<SearchQuote>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchQuote {
    symbol: Option<String>,
    quote_type: Option<String>,
    shortname: Option<String>,
    longname: Option<String>,
    exch_disp: Option<String>,
    is_yahoo_finance: Option<bool>,
}

impl SearchQuote {
    /// `None` for a result that is not a holdable listing with Yahoo data.
    fn hit(self) -> Option<SearchHit> {
        if self.is_yahoo_finance == Some(false) {
            return None;
        }
        let kind = kind(self.quote_type.as_deref()?)?;
        let symbol = self.symbol?;
        let name = name([self.longname, self.shortname], &symbol);
        Some(SearchHit {
            symbol,
            name,
            kind,
            exchange: self.exch_disp,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChartResponse {
    chart: Chart,
}

#[derive(Debug, Deserialize)]
struct Chart {
    result: Option<Vec<ChartResult>>,
}

#[derive(Debug, Deserialize)]
struct ChartResult {
    meta: ChartMeta,
    #[serde(default)]
    timestamp: Vec<i64>,
    #[serde(default)]
    indicators: Indicators,
}

#[derive(Debug, Default, Deserialize)]
struct Indicators {
    #[serde(default)]
    quote: Vec<ChartQuote>,
}

#[derive(Debug, Default, Deserialize)]
struct ChartQuote {
    #[serde(default)]
    close: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartMeta {
    symbol: String,
    currency: Option<String>,
    instrument_type: Option<String>,
    long_name: Option<String>,
    short_name: Option<String>,
    full_exchange_name: Option<String>,
    exchange_timezone_name: Option<String>,
}

impl ChartMeta {
    fn listing(self) -> Result<Listing, MarketError> {
        let quoted = self
            .currency
            .ok_or_else(|| MarketError::BadResponse(format!("{} has no currency", self.symbol)))?;
        let (currency, quote_currency) = currencies(&quoted).ok_or_else(|| {
            MarketError::BadResponse(format!("{} has currency {quoted:?}", self.symbol))
        })?;
        let kind = match self.instrument_type {
            Some(quote_type) => kind(&quote_type).ok_or(MarketError::Unsupported(quote_type))?,
            None => InstrumentKind::Other,
        };
        let name = name([self.long_name, self.short_name], &self.symbol);
        Ok(Listing {
            symbol: self.symbol,
            name,
            kind,
            currency,
            quote_currency,
            exchange: self.full_exchange_name,
            timezone: self.exchange_timezone_name,
        })
    }
}

/// The kind for a Yahoo quote type; `None` for what cannot be held.
fn kind(quote_type: &str) -> Option<InstrumentKind> {
    match quote_type {
        "EQUITY" => Some(InstrumentKind::Stock),
        "ETF" => Some(InstrumentKind::Etf),
        "MUTUALFUND" => Some(InstrumentKind::Fund),
        "CRYPTOCURRENCY" => Some(InstrumentKind::Crypto),
        _ => None,
    }
}

/// The first non-blank name, with runs of space collapsed; Yahoo pads some short
/// names. Falls back to the symbol.
fn name(candidates: [Option<String>; 2], symbol: &str) -> String {
    candidates
        .into_iter()
        .flatten()
        .map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| symbol.to_owned())
}

/// The spots in a spark answer, which maps each symbol to its recent closes.
#[allow(clippy::missing_docs_in_private_items)]
fn spark_spots(answer: &serde_json::Value) -> Result<Vec<Spot>, MarketError> {
    let mut spots = Vec::new();
    let parsed = answer.as_object().ok_or_else(|| {
        MarketError::BadResponse("the quote answer is not a map of symbols".to_owned())
    })?;
    for (symbol, series) in parsed {
        let timestamps = series
            .get("timestamp")
            .and_then(serde_json::Value::as_array);
        let closes = series.get("close").and_then(serde_json::Value::as_array);
        let (Some(timestamps), Some(closes)) = (timestamps, closes) else {
            continue;
        };
        let newest = timestamps
            .iter()
            .zip(closes)
            .rev()
            .find_map(|(at, close)| Some((at.as_i64()?, decimal_of(close)?)));
        let Some((at, close)) = newest else {
            continue;
        };
        spots.push(Spot {
            symbol: symbol.clone(),
            at: Timestamp::from_second(at)
                .map_err(|error| MarketError::BadResponse(error.to_string()))?,
            close,
        });
    }
    if spots.is_empty() {
        return Err(MarketError::BadResponse(
            "the quote answer holds no price series".to_owned(),
        ));
    }
    Ok(spots)
}

/// Reads a JSON number exactly: its own text, never a round trip through `f64`.
fn decimal_of(value: &serde_json::Value) -> Option<Decimal> {
    match value {
        serde_json::Value::Number(number) => Decimal::from_str(&number.to_string()).ok(),
        _ => None,
    }
}

/// The ISO 4217 currency and the quote currency for a Yahoo currency. Yahoo writes
/// minor units in mixed case: `GBp` is pence, quoted as `GBX`.
pub(crate) fn currencies(yahoo: &str) -> Option<(String, String)> {
    let (major, quoted) = match yahoo {
        "GBp" | "GBX" => ("GBP", "GBX"),
        "ZAc" | "ZAC" => ("ZAR", "ZAC"),
        "ILA" => ("ILS", "ILA"),
        code => (code, code),
    };
    let valid = |code: &str| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase());
    (valid(major) && valid(quoted)).then(|| (major.to_owned(), quoted.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pence_are_valued_in_pounds_and_quoted_in_pence() {
        assert_eq!(
            currencies("GBp"),
            Some(("GBP".to_owned(), "GBX".to_owned()))
        );
    }

    #[test]
    fn a_major_currency_is_both_value_and_quote() {
        assert_eq!(
            currencies("EUR"),
            Some(("EUR".to_owned(), "EUR".to_owned()))
        );
    }

    #[test]
    fn a_malformed_currency_is_refused() {
        assert_eq!(currencies("Eur"), None);
    }

    #[test]
    fn padded_names_are_collapsed() {
        let padded = Some("Nokia Oyj                     R".to_owned());

        assert_eq!(name([None, padded], "NOA3.DE"), "Nokia Oyj R");
    }

    #[test]
    fn a_blank_name_falls_back_to_the_symbol() {
        assert_eq!(name([Some("  ".to_owned()), None], "X.HE"), "X.HE");
    }

    #[test]
    fn a_wrapped_answer_is_a_bad_response() {
        let wrapped = serde_json::json!({ "spark": { "result": [] } });

        assert!(matches!(
            spark_spots(&wrapped),
            Err(MarketError::BadResponse(_))
        ));
    }

    #[test]
    fn a_series_without_a_close_is_left_out() {
        let answer = serde_json::json!({
            "NOKIA.HE": { "timestamp": [1], "close": [null] },
            "VOD.L": { "timestamp": [2], "close": [1.5] }
        });

        let spots = spark_spots(&answer).expect("spots");

        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].symbol, "VOD.L");
    }
}
