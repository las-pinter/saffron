//! What an instrument is and how it is identified.

use std::fmt;

/// The kind of an instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentKind {
    /// A company share.
    Stock,
    /// An exchange-traded fund.
    Etf,
    /// A mutual fund, priced once a day by its net asset value.
    Fund,
    /// A bond.
    Bond,
    /// A cryptocurrency.
    Crypto,
    /// Anything else that can be held.
    Other,
}

impl InstrumentKind {
    /// The stored and API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stock => "stock",
            Self::Etf => "etf",
            Self::Fund => "fund",
            Self::Bond => "bond",
            Self::Crypto => "crypto",
            Self::Other => "other",
        }
    }

    /// The kind named `name`, as [`Self::as_str`] writes it.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Stock,
            Self::Etf,
            Self::Fund,
            Self::Bond,
            Self::Crypto,
            Self::Other,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == name)
    }
}

impl fmt::Display for InstrumentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An ISIN (ISO 6166): two letters, nine letters or digits, and a check digit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Isin(String);

impl Isin {
    /// Parses `text` as an ISIN, ignoring surrounding space and letter case.
    /// Returns `None` when the shape or the check digit is wrong.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let code = text.trim().to_ascii_uppercase();
        let bytes = code.as_bytes();
        let shaped = bytes.len() == 12
            && bytes[..2].iter().all(u8::is_ascii_uppercase)
            && bytes[2..11].iter().all(u8::is_ascii_alphanumeric)
            && bytes[11].is_ascii_digit();
        (shaped && check_digit_holds(bytes)).then_some(Self(code))
    }

    /// The code in upper case.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Isin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The Luhn check over the code with each letter written as two digits (A = 10).
fn check_digit_holds(code: &[u8]) -> bool {
    let mut digits = Vec::with_capacity(24);
    for &byte in code {
        if byte.is_ascii_digit() {
            digits.push(u32::from(byte - b'0'));
        } else {
            let value = u32::from(byte - b'A') + 10;
            digits.extend([value / 10, value % 10]);
        }
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(position, &digit)| {
            if position % 2 == 1 {
                let doubled = digit * 2;
                doubled / 10 + doubled % 10
            } else {
                digit
            }
        })
        .sum();
    sum.is_multiple_of(10)
}
