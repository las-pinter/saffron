//! Yahoo Finance: symbol search and listing details.
//!
//! The endpoints are unofficial and need no account. Yahoo answers 429 to clients
//! without a browser-like `User-Agent`, so the client sends one. Requests are
//! spaced at least [`Yahoo::gap`] apart to stay well under its limits.

use std::time::Duration;

use saffron_core::instrument::InstrumentKind;
use serde::{Deserialize, de::DeserializeOwned};
use tokio::{sync::Mutex, time::Instant};
use url::Url;

use crate::{Listing, MarketError, SearchHit};

/// The name Saffron stores for this provider.
pub const PROVIDER: &str = "yahoo";
/// Where the live endpoints are.
pub const LIVE_URL: &str = "https://query2.finance.yahoo.com";
/// The shortest time between two requests to the live endpoints.
pub const LIVE_GAP: Duration = Duration::from_secs(1);
/// Most requests that may wait for their turn; more are refused as
/// [`MarketError::Busy`], so one burst cannot park every handler.
pub const MAX_WAITING: u32 = 4;
/// Most results one search returns.
pub const SEARCH_LIMIT: usize = 10;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
const TIMEOUT: Duration = Duration::from_secs(10);

/// A Yahoo Finance client.
#[derive(Debug)]
pub struct Yahoo {
    http: reqwest::Client,
    base: Url,
    gap: Duration,
    next_slot: Mutex<Instant>,
}

impl Yahoo {
    /// A client for the live endpoints.
    pub fn live() -> Result<Self, MarketError> {
        Self::new(LIVE_URL, LIVE_GAP)
    }

    /// A client for the endpoints at `base`, spacing requests `gap` apart.
    pub fn new(base: &str, gap: Duration) -> Result<Self, MarketError> {
        // reqwest is built without a TLS provider so that aws-lc stays out of the
        // build; ring is installed here. A second install fails harmlessly.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let base = Url::parse(base)
            .map_err(|error| MarketError::Unavailable(format!("bad Yahoo URL {base}: {error}")))?;
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .map_err(|error| MarketError::Unavailable(error.to_string()))?;
        Ok(Self {
            http,
            base,
            gap,
            next_slot: Mutex::new(Instant::now()),
        })
    }

    /// The shortest time between two requests.
    #[must_use]
    pub const fn gap(&self) -> Duration {
        self.gap
    }

    /// Listings matching an ISIN, a symbol, or a name. Kinds Saffron cannot hold,
    /// such as indices and currencies, are left out.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>, MarketError> {
        let mut url = self.url(&["v1", "finance", "search"]);
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("quotesCount", &SEARCH_LIMIT.to_string())
            .append_pair("newsCount", "0")
            .append_pair("listsCount", "0")
            .append_pair("enableFuzzyQuery", "false");
        let found: SearchResponse = self.fetch(url).await?;
        Ok(found
            .quotes
            .into_iter()
            .filter_map(SearchQuote::hit)
            .collect())
    }

    /// The listing `symbol`: its name, kind, currency, and exchange.
    pub async fn details(&self, symbol: &str) -> Result<Listing, MarketError> {
        let url = self.url(&["v8", "finance", "chart", symbol]);
        let chart: ChartResponse = self.fetch(url).await?;
        let meta = chart
            .chart
            .result
            .and_then(|results| results.into_iter().next())
            .ok_or(MarketError::NotFound)?
            .meta;
        meta.listing()
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("an http URL has path segments")
            .pop_if_empty()
            .extend(segments);
        url
    }

    async fn fetch<T: DeserializeOwned>(&self, url: Url) -> Result<T, MarketError> {
        self.wait_for_slot().await?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| MarketError::Unavailable(error.without_url().to_string()))?;
        match response.status().as_u16() {
            200..=299 => {}
            404 => return Err(MarketError::NotFound),
            429 => return Err(MarketError::RateLimited),
            status => return Err(MarketError::Unavailable(format!("HTTP {status}"))),
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| MarketError::Unavailable(error.without_url().to_string()))?;
        serde_json::from_slice(&body).map_err(|error| MarketError::BadResponse(error.to_string()))
    }

    async fn wait_for_slot(&self) -> Result<(), MarketError> {
        let start = {
            let mut next = self.next_slot.lock().await;
            let now = Instant::now();
            let start = (*next).max(now);
            if start > now + self.gap * MAX_WAITING {
                return Err(MarketError::Busy);
            }
            *next = start + self.gap;
            start
        };
        tokio::time::sleep_until(start).await;
        Ok(())
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

/// The ISO 4217 currency and the quote currency for a Yahoo currency. Yahoo writes
/// minor units in mixed case: `GBp` is pence, quoted as `GBX`.
fn currencies(yahoo: &str) -> Option<(String, String)> {
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
}
