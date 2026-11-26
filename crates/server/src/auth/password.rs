//! Argon2id password hashing.
//!
//! Hashing is slow by design, so the async helpers run it on the blocking pool.

use std::sync::LazyLock;

use argon2::{Argon2, PasswordHasher as _, PasswordVerifier as _};

/// Minimum password length in characters.
pub const MIN_CHARS: usize = 12;
/// Maximum password length in characters, which bounds hashing work per request.
pub const MAX_CHARS: usize = 1024;

/// A hash checked when the user does not exist, so an unknown username takes as long
/// to refuse as a wrong password.
static DECOY_HASH: LazyLock<Option<String>> =
    LazyLock::new(|| hash_now("saffron decoy password").ok());

/// Password failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasswordError {
    /// The password is shorter than [`MIN_CHARS`].
    #[error("the password must have at least {MIN_CHARS} characters")]
    TooShort,
    /// The password is longer than [`MAX_CHARS`].
    #[error("the password must have at most {MAX_CHARS} characters")]
    TooLong,
    /// Hashing failed.
    #[error("cannot hash the password: {0}")]
    Hash(String),
}

/// Checks the length rules.
pub fn validate(password: &str) -> Result<(), PasswordError> {
    let chars = password.chars().count();
    if chars < MIN_CHARS {
        return Err(PasswordError::TooShort);
    }
    if chars > MAX_CHARS {
        return Err(PasswordError::TooLong);
    }
    Ok(())
}

/// Validates and hashes `password` into a PHC string.
pub async fn hash(password: String) -> Result<String, PasswordError> {
    validate(&password)?;
    tokio::task::spawn_blocking(move || hash_now(&password))
        .await
        .map_err(|error| PasswordError::Hash(error.to_string()))?
}

/// Checks `password` against a stored PHC string; `None` checks the decoy and
/// always fails. An unreadable stored hash never matches.
pub async fn verify(password: String, stored: Option<String>) -> bool {
    let matched = tokio::task::spawn_blocking(move || {
        let Some(decoy) = DECOY_HASH.as_deref() else {
            return false;
        };
        let target = stored.as_deref().unwrap_or(decoy);
        let matched = verify_now(&password, target);
        stored.is_some() && matched
    })
    .await;
    matched.unwrap_or(false)
}

fn hash_now(password: &str) -> Result<String, PasswordError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| PasswordError::Hash(error.to_string()))
}

fn verify_now(password: &str, stored: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "correct horse battery";

    #[tokio::test]
    async fn a_hash_verifies_its_password() {
        let stored = hash(PASSWORD.to_owned()).await.expect("hash");

        assert!(stored.starts_with("$argon2id$"), "hash: {stored}");
        assert!(verify(PASSWORD.to_owned(), Some(stored)).await);
    }

    #[tokio::test]
    async fn a_wrong_password_does_not_verify() {
        let stored = hash(PASSWORD.to_owned()).await.expect("hash");

        assert!(!verify("wrong password here".to_owned(), Some(stored)).await);
    }

    #[tokio::test]
    async fn a_missing_user_never_verifies_even_with_the_decoy_password() {
        assert!(!verify("saffron decoy password".to_owned(), None).await);
    }

    #[tokio::test]
    async fn an_unreadable_hash_never_matches() {
        assert!(!verify(PASSWORD.to_owned(), Some("not a phc string".to_owned())).await);
    }

    #[test]
    fn eleven_characters_are_too_short() {
        assert_eq!(
            validate(&"a".repeat(MIN_CHARS - 1)),
            Err(PasswordError::TooShort)
        );
    }

    #[test]
    fn eleven_multibyte_characters_are_too_short() {
        assert_eq!(
            validate(&"é".repeat(MIN_CHARS - 1)),
            Err(PasswordError::TooShort)
        );
    }

    #[test]
    fn twelve_multibyte_characters_are_enough() {
        assert_eq!(validate(&"é".repeat(MIN_CHARS)), Ok(()));
    }

    #[test]
    fn more_than_the_maximum_is_too_long() {
        assert_eq!(
            validate(&"a".repeat(MAX_CHARS + 1)),
            Err(PasswordError::TooLong)
        );
    }
}
