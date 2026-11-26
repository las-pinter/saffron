//! Market data for Saffron: price, FX, and news providers.

pub mod yahoo;

#[cfg(feature = "test-support")]
pub mod mock;

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
}
