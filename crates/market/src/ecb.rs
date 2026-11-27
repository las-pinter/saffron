//! The European Central Bank's daily reference rates, quoted as units of a
//! currency per one euro.
//!
//! Today's rates come as a small XML file. The full history is a CSV inside a zip
//! archive; the plain CSV URL serves a stale copy (it stops in 2010), so it is not
//! used. Rates are published once each working day, about 16:00 CET; there is no
//! rate for weekends and holidays.

use std::{collections::BTreeMap, io::Read, str::FromStr, time::Duration};

use jiff::civil::Date;
use rust_decimal::Decimal;

use crate::{MarketError, client::Client};

/// Where the live files are.
pub const LIVE_URL: &str = "https://www.ecb.europa.eu";
/// The shortest time between two requests; the ECB files are large and rarely
/// fetched.
pub const LIVE_GAP: Duration = Duration::from_secs(1);
/// The daily file.
pub const DAILY_PATH: &str = "stats/eurofxref/eurofxref-daily.xml";
/// The full history, a zip holding `eurofxref-hist.csv`.
pub const HISTORY_PATH: &str = "stats/eurofxref/eurofxref-hist.zip";
/// Most bytes one file may carry, decompressed; the live archive is about 2 MB.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// One working day's rates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rates {
    /// The day the rates were published for.
    pub date: Date,
    /// Units of each currency per one euro, keyed by ISO 4217 code.
    pub per_eur: BTreeMap<String, Decimal>,
}

/// An ECB client.
#[derive(Debug)]
pub struct Ecb {
    client: Client,
}

impl Ecb {
    /// A client for the live files.
    pub fn live() -> Result<Self, MarketError> {
        Self::new(LIVE_URL, LIVE_GAP)
    }

    /// A client for the files at `base`, spacing requests `gap` apart.
    pub fn new(base: &str, gap: Duration) -> Result<Self, MarketError> {
        Ok(Self {
            client: Client::new(base, gap)?,
        })
    }

    /// The latest published day's rates.
    pub async fn daily(&self) -> Result<Rates, MarketError> {
        let url = self.client.url(&DAILY_PATH.split('/').collect::<Vec<_>>());
        let xml = String::from_utf8(self.client.get_bytes(url).await?)
            .map_err(|_| MarketError::BadResponse("the daily file is not UTF-8".to_owned()))?;
        let mut rates = parse_daily(&xml)?;
        rates
            .pop()
            .ok_or_else(|| MarketError::BadResponse("the daily file holds no rates".to_owned()))
    }

    /// Every day the ECB has published, newest first.
    pub async fn history(&self) -> Result<Vec<Rates>, MarketError> {
        let url = self
            .client
            .url(&HISTORY_PATH.split('/').collect::<Vec<_>>());
        let archive = self.client.get_bytes(url).await?;
        if archive.len() as u64 > MAX_BYTES {
            return Err(MarketError::BadResponse(
                "the history archive is larger than expected".to_owned(),
            ));
        }
        parse_archive(&archive)
    }
}

/// Reads the `<Cube time=...>` day in the daily file.
fn parse_daily(xml: &str) -> Result<Vec<Rates>, MarketError> {
    let mut days = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Cube time=") {
        let after = &rest[start..];
        let time = attribute(after, "time").ok_or_else(|| {
            MarketError::BadResponse("a day in the daily file has no time".to_owned())
        })?;
        let body_start = after.find('>').map_or(after.len(), |index| index + 1);
        let body = &after[body_start..];
        let body_end = body.find("</Cube>").unwrap_or(body.len());
        let day = Rates {
            date: time.parse().map_err(|error| {
                MarketError::BadResponse(format!("bad ECB date {time}: {error}"))
            })?,
            per_eur: rates_of(&body[..body_end])?,
        };
        days.push(day);
        rest = &body[body_end..];
    }
    Ok(days)
}

/// Every `<Cube currency=... rate=.../>` in `xml`.
fn rates_of(xml: &str) -> Result<BTreeMap<String, Decimal>, MarketError> {
    let mut rates = BTreeMap::new();
    for entry in xml.split("<Cube ").skip(1) {
        if !entry.starts_with("currency=") {
            continue;
        }
        let tag = format!("<Cube {entry}");
        let currency = attribute(&tag, "currency")
            .ok_or_else(|| MarketError::BadResponse("a rate has no currency".to_owned()))?;
        let rate = attribute(&tag, "rate")
            .and_then(|text| Decimal::from_str(&text).ok())
            .filter(|rate| *rate > Decimal::ZERO)
            .ok_or_else(|| MarketError::BadResponse(format!("{currency} has no usable rate")))?;
        rates.insert(currency, rate);
    }
    Ok(rates)
}

/// The value of `name` in the first `<...>` tag of `xml`, in either quote style.
fn attribute(xml: &str, name: &str) -> Option<String> {
    let tag_end = xml.find('>')?;
    let tag = &xml[..tag_end];
    let start = tag.find(&format!("{name}="))? + name.len() + 1;
    let quote = tag[start..].chars().next()?;
    let rest = &tag[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

/// Reads the CSV inside the history archive.
fn parse_archive(archive: &[u8]) -> Result<Vec<Rates>, MarketError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|error| {
        MarketError::BadResponse(format!("the history archive is unreadable: {error}"))
    })?;
    let mut entry = zip.by_name("eurofxref-hist.csv").map_err(|error| {
        MarketError::BadResponse(format!("the history CSV is missing: {error}"))
    })?;
    let mut csv = String::new();
    entry.read_to_string(&mut csv).map_err(|error| {
        MarketError::BadResponse(format!("the history CSV is unreadable: {error}"))
    })?;
    let mut days = parse_csv(&csv)?;
    days.sort_by_key(|day| std::cmp::Reverse(day.date));
    Ok(days)
}

/// Reads the history CSV: a header of currency codes, then one line per day.
fn parse_csv(csv: &str) -> Result<Vec<Rates>, MarketError> {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines
        .next()
        .ok_or_else(|| MarketError::BadResponse("the history CSV is empty".to_owned()))?
        .split(',')
        .collect();
    if header.first() != Some(&"Date") {
        return Err(MarketError::BadResponse(
            "the history CSV has no Date column".to_owned(),
        ));
    }
    let mut days = Vec::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let mut cells = line.split(',');
        let date_text = cells.next().unwrap_or_default();
        let date = date_text.parse().map_err(|error| {
            MarketError::BadResponse(format!("bad ECB date {date_text}: {error}"))
        })?;
        let mut per_eur = BTreeMap::new();
        for (code, cell) in header.iter().skip(1).zip(cells) {
            let value = cell.trim();
            if value.is_empty() || value == "N/A" {
                continue;
            }
            if code.len() != 3 {
                continue;
            }
            if let Ok(rate) = Decimal::from_str(value)
                && rate > Decimal::ZERO
            {
                per_eur.insert((*code).to_owned(), rate);
            }
        }
        if !per_eur.is_empty() {
            days.push(Rates { date, per_eur });
        }
    }
    Ok(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_of(csv: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("eurofxref-hist.csv", options)
            .expect("entry");
        std::io::Write::write_all(&mut writer, csv.as_bytes()).expect("write");
        writer.finish().expect("finish").into_inner()
    }

    #[test]
    fn history_is_newest_first_even_when_the_file_is_not() {
        let csv = "Date,USD\n2026-11-24,1.09\n2026-11-26,1.1\n2026-11-25,1.095\n";

        let days = parse_archive(&archive_of(csv)).expect("days");

        let dates: Vec<String> = days.iter().map(|day| day.date.to_string()).collect();
        assert_eq!(dates, ["2026-11-26", "2026-11-25", "2026-11-24"]);
    }

    #[test]
    fn a_row_without_any_rate_is_left_out() {
        let csv = "Date,USD\n2026-11-26,N/A\n";

        let days = parse_archive(&archive_of(csv)).expect("days");

        assert_eq!(days, Vec::new());
    }
}
