//! State shared by every request.

use std::sync::Arc;

use sqlx::SqlitePool;

use tokio::sync::{Mutex, Semaphore};

use crate::{
    auth::{client::TrustedProxy, origin::PublicOrigin, setup::SetupToken},
    clock::Clock,
    secrets::SecretBox,
};

/// Handles to the database and the services the handlers need.
#[derive(Debug, Clone)]
pub struct AppState {
    /// The database.
    pub pool: SqlitePool,
    /// The origin the browser uses.
    pub origin: Arc<PublicOrigin>,
    /// The source of the current time.
    pub clock: Clock,
    /// Seals secrets stored in the database.
    pub secrets: Arc<SecretBox>,
    /// The first-run setup token.
    pub setup: Arc<SetupToken>,
    /// Proxies whose `X-Forwarded-For` header names the client.
    pub trusted_proxies: Arc<[TrustedProxy]>,
    /// Held while a password is checked and the outcome recorded, so parallel
    /// guesses cannot overshoot the lockout, and only one Argon2 verification runs at
    /// a time.
    pub sign_in_gate: Arc<Mutex<()>>,
    /// Bounds how many sign-ins may wait for [`Self::sign_in_gate`].
    pub sign_in_queue: Arc<Semaphore>,
}

/// Sign-ins that may wait for their turn at once; more are refused with 429.
pub const MAX_QUEUED_SIGN_INS: usize = 16;

impl AppState {
    /// State over `pool` for `origin`, sealing secrets with `secrets`.
    #[must_use]
    pub fn new(pool: SqlitePool, origin: PublicOrigin, clock: Clock, secrets: SecretBox) -> Self {
        Self {
            pool,
            origin: Arc::new(origin),
            clock,
            secrets: Arc::new(secrets),
            setup: Arc::new(SetupToken::default()),
            trusted_proxies: Arc::new([]),
            sign_in_gate: Arc::new(Mutex::new(())),
            sign_in_queue: Arc::new(Semaphore::new(MAX_QUEUED_SIGN_INS)),
        }
    }

    /// Believes `X-Forwarded-For` from `proxies`.
    #[must_use]
    pub fn with_trusted_proxies(mut self, proxies: Vec<TrustedProxy>) -> Self {
        self.trusted_proxies = proxies.into();
        self
    }
}
