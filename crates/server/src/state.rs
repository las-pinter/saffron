//! State shared by every request.

use std::sync::Arc;

use jiff::tz::TimeZone;
use saffron_market::Providers;
use sqlx::SqlitePool;

use tokio::sync::{Mutex, Semaphore};
use webauthn_rs::prelude::Webauthn;

use crate::{
    auth::{
        ceremony::CeremonyStore, client::TrustedProxy, origin::PublicOrigin, setup::SetupToken,
    },
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
    /// The passkey relying party; `None` when the origin cannot have one.
    pub webauthn: Option<Arc<Webauthn>>,
    /// Passkey ceremonies between their start and finish calls.
    pub ceremonies: CeremonyStore,
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
    /// The market data providers.
    pub market: Arc<Providers>,
    /// The time zone of scheduled jobs such as the daily fund refresh.
    pub timezone: TimeZone,
    /// Held while a book write reads the portfolio, checks it, and stores the row,
    /// so two overlapping writes cannot both pass the check and break the book.
    pub book_writes: Arc<Mutex<()>>,
}

/// The time zone of scheduled jobs unless `SAFFRON_TIMEZONE` names another.
pub const DEFAULT_TIMEZONE: &str = "Europe/Helsinki";

/// Sign-ins that may wait for their turn at once; more are refused with 429.
pub const MAX_QUEUED_SIGN_INS: usize = 16;

impl AppState {
    /// State over `pool` for `origin`, sealing secrets with `secrets` and reading
    /// market data from `market`. Scheduled jobs run in [`DEFAULT_TIMEZONE`].
    #[must_use]
    pub fn new(
        pool: SqlitePool,
        origin: PublicOrigin,
        clock: Clock,
        secrets: SecretBox,
        market: Providers,
    ) -> Self {
        Self {
            pool,
            webauthn: origin.relying_party().map(Arc::new),
            ceremonies: CeremonyStore::default(),
            origin: Arc::new(origin),
            clock,
            secrets: Arc::new(secrets),
            setup: Arc::new(SetupToken::default()),
            trusted_proxies: Arc::new([]),
            sign_in_gate: Arc::new(Mutex::new(())),
            sign_in_queue: Arc::new(Semaphore::new(MAX_QUEUED_SIGN_INS)),
            market: Arc::new(market),
            timezone: TimeZone::get(DEFAULT_TIMEZONE).unwrap_or(TimeZone::UTC),
            book_writes: Arc::new(Mutex::new(())),
        }
    }

    /// Runs scheduled jobs in `timezone`.
    #[must_use]
    pub fn with_timezone(mut self, timezone: TimeZone) -> Self {
        self.timezone = timezone;
        self
    }

    /// Believes `X-Forwarded-For` from `proxies`.
    #[must_use]
    pub fn with_trusted_proxies(mut self, proxies: Vec<TrustedProxy>) -> Self {
        self.trusted_proxies = proxies.into();
        self
    }
}
