//! FT markets: daily fund prices (NAVs), the fallback when Yahoo has none.
//!
//! The endpoints are undocumented; they serve the markets.ft.com pages and need
//! no account. A fund is found by ISIN, which gives its FT symbol
//! (`FI4000315700:EUR`) and its internal id (`xid`); the history endpoint takes
//! the id and answers with HTML table rows inside JSON.

use std::{str::FromStr, time::Duration};

use jiff::{
    civil::{Date, Time},
    tz::TimeZone,
};
use rust_decimal::Decimal;
use saffron_core::instrument::Isin;
use serde::Deserialize;

use crate::{DailyClose, MarketError, Quote, client::Client};

/// The name Saffron stores for this provider.
pub const PROVIDER: &str = "ft";
/// Where the live endpoints are.
pub const LIVE_URL: &str = "https://markets.ft.com";
/// The shortest time between two requests to the live endpoints.
pub const LIVE_GAP: Duration = Duration::from_secs(1);
/// How far back [`Ft::latest`] looks for the newest NAV.
pub const LATEST_WINDOW_DAYS: i64 = 14;

/// A fund as FT lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtFund {
    /// FT's symbol, `<ISIN>:<currency>`.
    pub symbol: String,
    /// FT's internal id, which the history endpoint takes.
    pub xid: String,
    /// The fund's name.
    pub name: String,
    /// The ISO 4217 currency of its NAV.
    pub currency: String,
}

/// An FT markets client.
#[derive(Debug)]
pub struct Ft {
    client: Client,
}

impl Ft {
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

    /// The fund with `isin`. FT's search is fuzzy, so only a result whose symbol
    /// is that ISIN counts.
    pub async fn find_fund(&self, isin: &Isin) -> Result<FtFund, MarketError> {
        let mut url = self.client.url(&["data", "searchapi", "searchsecurities"]);
        url.query_pairs_mut().append_pair("query", isin.as_str());
        let found: SearchResponse = self.client.get_json(url).await?;
        let prefix = format!("{}:", isin.as_str());
        let security = found
            .data
            .security
            .into_iter()
            .find(|security| security.symbol.starts_with(&prefix))
            .ok_or(MarketError::NotFound)?;
        let currency = security
            .symbol
            .rsplit(':')
            .next()
            .filter(|code| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()))
            .ok_or_else(|| {
                MarketError::BadResponse(format!("FT symbol {} names no currency", security.symbol))
            })?
            .to_owned();
        Ok(FtFund {
            symbol: security.symbol,
            xid: security.xid,
            name: security.name,
            currency,
        })
    }

    /// The fund's NAVs from `from` to `to`, oldest first.
    pub async fn history(
        &self,
        fund: &FtFund,
        from: Date,
        to: Date,
    ) -> Result<Vec<DailyClose>, MarketError> {
        let mut url = self
            .client
            .url(&["data", "equities", "ajax", "get-historical-prices"]);
        url.query_pairs_mut()
            .append_pair("startDate", &from.strftime("%Y/%m/%d").to_string())
            .append_pair("endDate", &to.strftime("%Y/%m/%d").to_string())
            .append_pair("symbol", &fund.xid);
        let answer: HistoryResponse = self.client.get_json(url).await?;
        let mut closes = parse_rows(&answer.html)?;
        closes.sort_by_key(|close| close.date);
        closes.dedup_by_key(|close| close.date);
        Ok(closes)
    }

    /// The fund's newest NAV in the [`LATEST_WINDOW_DAYS`] up to `today`.
    pub async fn latest(&self, fund: &FtFund, today: Date) -> Result<Quote, MarketError> {
        let from = today
            .checked_sub(jiff::Span::new().days(LATEST_WINDOW_DAYS))
            .map_err(|error| MarketError::BadResponse(error.to_string()))?;
        let newest = self
            .history(fund, from, today)
            .await?
            .pop()
            .ok_or(MarketError::NotFound)?;
        // FT gives a NAV date, not a time: the price counts from that day's start.
        let as_of = TimeZone::UTC
            .to_timestamp(newest.date.to_datetime(Time::midnight()))
            .map_err(|error| MarketError::BadResponse(error.to_string()))?;
        Ok(Quote {
            symbol: fund.symbol.clone(),
            date: newest.date,
            close: newest.close,
            currency: fund.currency.clone(),
            as_of,
            timezone: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: SearchData,
}

#[derive(Debug, Default, Deserialize)]
struct SearchData {
    #[serde(default)]
    security: Vec<Security>,
}

#[derive(Debug, Deserialize)]
struct Security {
    symbol: String,
    xid: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct HistoryResponse {
    #[serde(default)]
    html: String,
}

/// Reads `<tr>` rows of date, open, high, low, close, volume. The date cell
/// holds a long form (`Wednesday, November 25, 2026`) and a short form; the long
/// one comes first.
/// Reads every price row, skipping rows that do not hold a price. Markup with
/// rows but no readable price is an error, so a changed page cannot look like a
/// fund without NAVs.
fn parse_rows(html: &str) -> Result<Vec<DailyClose>, MarketError> {
    let candidates: Vec<&str> = html
        .split("<tr>")
        .filter(|row| row.contains("<td"))
        .collect();
    let closes: Vec<DailyClose> = candidates
        .iter()
        .filter_map(|row| parse_row(row).ok())
        .collect();
    if !candidates.is_empty() && closes.is_empty() {
        return Err(MarketError::BadResponse(
            "no readable price row in the FT history".to_owned(),
        ));
    }
    Ok(closes)
}

fn parse_row(row: &str) -> Result<DailyClose, MarketError> {
    let cells: Vec<String> = row
        .split("<td")
        .skip(1)
        .map(|cell| first_text(cell.split_once('>').map_or("", |(_, rest)| rest)))
        .collect();
    let bad = || MarketError::BadResponse(format!("unreadable FT price row: {row:.200}"));
    let [date, _, _, _, close, ..] = cells.as_slice() else {
        return Err(bad());
    };
    let date = jiff::fmt::strtime::parse("%A, %B %d, %Y", date)
        .and_then(|parsed| parsed.to_date())
        .map_err(|_| bad())?;
    let close = Decimal::from_str(&close.replace(',', "")).map_err(|_| bad())?;
    Ok(DailyClose { date, close })
}

/// The first run of text in `html`, outside any tag.
fn first_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                if !text.trim().is_empty() {
                    break;
                }
                in_tag = true;
            }
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_long_date_is_read_from_a_row() {
        let row = r#"<td class="x"><span class="a">Monday, March 2, 2026</span><span class="b">Mon, Mar 2, 2026</span></td><td>1</td><td>1</td><td>1</td><td>1,234.50</td><td>0</td></tr>"#;

        let parsed = parse_row(row).expect("row");

        assert_eq!(
            parsed,
            DailyClose {
                date: jiff::civil::date(2026, 3, 2),
                close: Decimal::from_str("1234.50").expect("decimal"),
            }
        );
    }

    #[test]
    fn a_short_row_is_a_bad_response() {
        let row = "<td>Monday, March 2, 2026</td><td>1</td></tr>";

        assert!(matches!(parse_row(row), Err(MarketError::BadResponse(_))));
    }

    #[test]
    fn rows_that_do_not_parse_make_the_page_a_bad_response() {
        let html = "<tr><td>nonsense</td></tr><tr><td>more nonsense</td></tr>";

        assert!(matches!(parse_rows(html), Err(MarketError::BadResponse(_))));
    }

    #[test]
    fn an_unreadable_row_beside_a_good_one_is_skipped() {
        let html = concat!(
            "<tr><td>Monday, March 2, 2026</td><td>1</td><td>1</td><td>1</td><td>2.5</td><td>0</td></tr>",
            "<tr><td>nonsense</td></tr>"
        );

        assert_eq!(parse_rows(html).expect("rows").len(), 1);
    }
}
