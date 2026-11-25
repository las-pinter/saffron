//! Decimal columns.
//!
//! SQLite has no decimal type and REAL loses cents, so amounts and quantities are
//! stored as TEXT. Writing always goes through [`to_text`], which produces one
//! canonical form per value, so equal values compare equal in SQL too.

use rust_decimal::Decimal;
use thiserror::Error;

/// A stored decimal that does not parse.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("`{text}` is not a valid decimal: {reason}")]
pub struct InvalidDecimal {
    /// The stored text.
    pub text: String,
    /// Why parsing failed.
    pub reason: String,
}

/// The canonical text of `value`: no exponent and no trailing fractional zeros
/// (`1.50` → `1.5`, `2.000` → `2`, `-0` → `0`).
#[must_use]
pub fn to_text(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Parses a stored decimal. Scientific notation is refused, since [`to_text`] never
/// writes it.
pub fn from_text(text: &str) -> Result<Decimal, InvalidDecimal> {
    let invalid = |reason: String| InvalidDecimal {
        text: text.to_owned(),
        reason,
    };
    // rust_decimal also accepts exponents and `_` separators; stored text never has them.
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(invalid(
            "only digits, one `.`, and a leading `-` are allowed".into(),
        ));
    }
    text.parse::<Decimal>()
        .map_err(|error| invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn dec(text: &str) -> Decimal {
        Decimal::from_str(text).expect("valid test decimal")
    }

    #[test]
    fn round_trip_preserves_value() {
        for text in [
            "0",
            "0.01",
            "-0.01",
            "4.21",
            "0.000000000001",
            "123456789012345678.123456789",
            "-98765.4321",
            "79228162514264337593543950335",
        ] {
            let value = dec(text);

            let stored = to_text(value);

            assert_eq!(
                from_text(&stored),
                Ok(value),
                "value {text} stored as {stored}"
            );
        }
    }

    #[test]
    fn to_text_drops_trailing_fractional_zeros() {
        assert_eq!(to_text(dec("1.50")), "1.5");
    }

    #[test]
    fn to_text_writes_whole_numbers_without_a_point() {
        assert_eq!(to_text(dec("2.000")), "2");
    }

    #[test]
    fn to_text_writes_negative_zero_as_zero() {
        assert_eq!(to_text(dec("-0.00")), "0");
    }

    #[test]
    fn equal_values_get_equal_text() {
        assert_eq!(to_text(dec("10.10")), to_text(dec("10.1000")));
    }

    #[test]
    fn from_text_rejects_garbage() {
        let error = from_text("12,50").expect_err("comma is not a decimal point");

        assert_eq!(error.text, "12,50");
    }

    #[test]
    fn from_text_rejects_scientific_notation() {
        assert!(from_text("1e3").is_err());
    }

    #[test]
    fn from_text_rejects_digit_separators() {
        assert!(from_text("1_000").is_err());
    }

    #[test]
    fn from_text_rejects_empty_text() {
        assert!(from_text("").is_err());
    }

    #[test]
    fn from_text_rejects_two_points() {
        assert!(from_text("1.2.3").is_err());
    }
}
