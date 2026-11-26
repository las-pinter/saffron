//! Random secret tokens and their at-rest hashes.
//!
//! Session and setup tokens are secrets. Only their SHA-256 is stored, so a copy of
//! the database cannot be replayed against the API.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// Entropy of a token in bytes.
const TOKEN_BYTES: usize = 32;

/// The SHA-256 of a token.
pub type TokenHash = [u8; 32];

/// The operating system random source failed.
#[derive(Debug, thiserror::Error)]
#[error("cannot read random bytes: {0}")]
pub struct RandomError(String);

/// A fresh 256-bit token as unpadded base64url (43 characters).
pub fn random_token() -> Result<String, RandomError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| RandomError(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// The SHA-256 of `token`.
#[must_use]
pub fn hash_token(token: &str) -> TokenHash {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn tokens_are_url_safe_and_43_characters() {
        let token = random_token().expect("random token");

        assert_eq!(token.len(), 43);
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token: {token}"
        );
    }

    #[test]
    fn tokens_are_unique() {
        let tokens: HashSet<String> = (0..32)
            .map(|_| random_token().expect("random token"))
            .collect();

        assert_eq!(tokens.len(), 32);
    }

    #[test]
    fn hashes_are_stable_and_differ_per_token() {
        assert_eq!(hash_token("one"), hash_token("one"));
        assert_ne!(hash_token("one"), hash_token("two"));
    }
}
