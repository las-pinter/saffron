//! Market data for Saffron: price, FX, and news providers.

mod client;
pub mod ecb;
pub mod ft;
pub mod yahoo;

#[cfg(feature = "test-support")]
pub mod mock;

use jiff::{Timestamp, civil::Date};
use rust_decimal::Decimal;
use saffron_core::instrument::InstrumentKind;

/// Why a provider could not answer.
#[derive(Debug, thiserror::Error)]
pub enum MarketError {
    /// The provider does not know the symbol.
    #[error("the provider does not know this symbol")]
    NotFound,
    /// The provider refused because of too many requests.
    #[error("the provider is refusing requests for now (rate limited)")]
    RateLimited,
    /// Too many requests are already waiting for this provider.
    #[error("too many requests are waiting for the provider")]
    Busy,
    /// The listing is of a kind Saffron does not hold, such as an index.
    #[error("the provider lists this as {0}, which Saffron does not hold")]
    Unsupported(String),
    /// The provider could not be reached or failed.
    #[error("the provider is unavailable: {0}")]
    Unavailable(String),
    /// The provider answered with something Saffron cannot read.
    #[error("the provider answered in an unexpected form: {0}")]
    BadResponse(String),
}

/// One search result: a listing the user may pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// The provider's symbol, such as `NOKIA.HE`.
    pub symbol: String,
    /// The instrument's name.
    pub name: String,
    /// What it is.
    pub kind: InstrumentKind,
    /// The exchange as the provider shows it, such as `Helsinki`.
    pub exchange: Option<String>,
}

/// A listing as the provider describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The provider's symbol.
    pub symbol: String,
    /// The instrument's name.
    pub name: String,
    /// What it is.
    pub kind: InstrumentKind,
    /// The ISO 4217 currency the instrument is valued in, such as `GBP`.
    pub currency: String,
    /// The currency the provider quotes prices in. It differs from
    /// [`Self::currency`] for minor units, such as `GBX` (pence) for `GBP`.
    pub quote_currency: String,
    /// The exchange as the provider shows it.
    pub exchange: Option<String>,
    /// The exchange's IANA time zone, when the provider names it.
    pub timezone: Option<String>,
}

/// The latest price of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    /// The provider's symbol.
    pub symbol: String,
    /// The trading day the price belongs to, in the exchange's time zone.
    pub date: Date,
    /// The price in [`Self::currency`]; minor units are already converted, so a
    /// price quoted as 250 pence reads 2.50 GBP.
    pub close: Decimal,
    /// The ISO 4217 currency of [`Self::close`].
    pub currency: String,
    /// When the provider says the price was valid.
    pub as_of: Timestamp,
    /// The exchange's IANA time zone, when the provider names it.
    pub timezone: Option<String>,
}

/// A price the provider reports, before the minor unit is converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    /// The provider's symbol.
    pub symbol: String,
    /// When the price was reported.
    pub at: Timestamp,
    /// The price as quoted, so pence for a `GBp` listing.
    pub close: Decimal,
}

/// The major currency of `quoted` and the price in it: 250 in `GBp` reads
/// (`GBP`, 2.5).
#[must_use]
pub fn normalize(quoted: &str, close: Decimal) -> Option<(String, Decimal)> {
    let (major, minor) = yahoo::currencies(quoted)?;
    let divisor = if minor == major {
        Decimal::ONE
    } else {
        Decimal::ONE_HUNDRED
    };
    Some((major, close / divisor))
}

/// Daily closing prices of one listing, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct History {
    /// The provider's symbol.
    pub symbol: String,
    /// The ISO 4217 currency of every close; minor units are already converted.
    pub currency: String,
    /// One close per trading day.
    pub closes: Vec<DailyClose>,
}

/// The close of one trading day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyClose {
    /// The trading day.
    pub date: Date,
    /// The closing price, or the net asset value for a fund.
    pub close: Decimal,
}

/// Every provider Saffron reads prices from.
#[derive(Debug)]
pub struct Providers {
    /// The primary source.
    pub yahoo: yahoo::Yahoo,
    /// The fund fallback.
    pub ft: ft::Ft,
    /// ECB reference rates.
    pub ecb: ecb::Ecb,
}

impl Providers {
    /// Clients for the live endpoints.
    pub fn live() -> Result<Self, MarketError> {
        Ok(Self {
            yahoo: yahoo::Yahoo::live()?,
            ft: ft::Ft::live()?,
            ecb: ecb::Ecb::live()?,
        })
    }
}
