//! The first-run setup token.
//!
//! While no user exists, the server prints a one-time link to its log at startup.
//! Only someone who can read the log can create the first administrator, so a
//! stranger who reaches a fresh server first cannot claim it.

use std::sync::{Mutex, PoisonError};

use super::token::{RandomError, TokenHash, hash_token, random_token};

/// The current setup token, kept only as a hash and only in memory.
#[derive(Debug, Default)]
pub struct SetupToken {
    hash: Mutex<Option<TokenHash>>,
}

impl SetupToken {
    /// Issues a new token, replacing any earlier one, and returns it in clear text.
    pub fn issue(&self) -> Result<String, RandomError> {
        let token = random_token()?;
        *self.lock() = Some(hash_token(&token));
        Ok(token)
    }

    /// Whether `token` is the current setup token.
    #[must_use]
    pub fn matches(&self, token: &str) -> bool {
        *self.lock() == Some(hash_token(token))
    }

    /// Retires the token after setup.
    pub fn clear(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<TokenHash>> {
        self.hash.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_token_matches_before_one_is_issued() {
        assert!(!SetupToken::default().matches(""));
    }

    #[test]
    fn the_issued_token_matches() {
        let setup = SetupToken::default();

        let token = setup.issue().expect("issue");

        assert!(setup.matches(&token));
    }

    #[test]
    fn a_new_token_replaces_the_old_one() {
        let setup = SetupToken::default();
        let first = setup.issue().expect("issue");

        setup.issue().expect("reissue");

        assert!(!setup.matches(&first));
    }

    #[test]
    fn a_cleared_token_no_longer_matches() {
        let setup = SetupToken::default();
        let token = setup.issue().expect("issue");

        setup.clear();

        assert!(!setup.matches(&token));
    }
}
