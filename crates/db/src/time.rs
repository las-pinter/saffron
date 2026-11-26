//! Timestamp columns.
//!
//! Timestamps are stored as RFC 3339 UTC text with whole seconds and a `Z` suffix.
//! One fixed width means text order is time order, so SQL can compare them.

use jiff::Timestamp;
use thiserror::Error;

/// A stored timestamp that does not parse.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("`{text}` is not an RFC 3339 timestamp: {reason}")]
pub struct InvalidTimestamp {
    /// The stored text.
    pub text: String,
    /// Why parsing failed.
    pub reason: String,
}

/// The stored text of `value`, truncated to whole seconds.
#[must_use]
pub fn to_text(value: Timestamp) -> String {
    value.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Parses a stored timestamp.
pub fn from_text(text: &str) -> Result<Timestamp, InvalidTimestamp> {
    text.parse::<Timestamp>().map_err(|error| InvalidTimestamp {
        text: text.to_owned(),
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().expect("valid test timestamp")
    }

    #[test]
    fn to_text_writes_utc_with_whole_seconds() {
        assert_eq!(
            to_text(at("2026-11-25T14:30:05.987+02:00")),
            "2026-11-25T12:30:05Z"
        );
    }

    #[test]
    fn to_text_keeps_a_fixed_width_on_whole_seconds() {
        assert_eq!(to_text(at("2026-01-02T03:04:05Z")), "2026-01-02T03:04:05Z");
    }

    #[test]
    fn text_order_never_contradicts_time_order() {
        // A fraction would sort `…:00.5Z` before `…:00Z`, since `.` sorts before `Z`.
        let earlier = to_text(at("2026-11-25T12:00:00Z"));
        let later = to_text(at("2026-11-25T12:00:00.5Z"));

        assert!(earlier <= later, "{earlier} <= {later}");
    }

    #[test]
    fn from_text_reads_what_to_text_writes() {
        let value = at("2026-11-25T12:30:05Z");

        assert_eq!(from_text(&to_text(value)), Ok(value));
    }

    #[test]
    fn from_text_rejects_garbage() {
        let error = from_text("yesterday").expect_err("not a timestamp");

        assert_eq!(error.text, "yesterday");
    }
}
