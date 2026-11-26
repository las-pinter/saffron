//! The source of the current time, replaceable in tests.

use std::{fmt, sync::Arc};

use jiff::Timestamp;

/// Returns the current time.
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> Timestamp + Send + Sync>);

impl Clock {
    /// The system clock.
    #[must_use]
    pub fn system() -> Self {
        Self(Arc::new(Timestamp::now))
    }

    /// A clock that asks `now` for the time.
    pub fn from_fn(now: impl Fn() -> Timestamp + Send + Sync + 'static) -> Self {
        Self(Arc::new(now))
    }

    /// The current time.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        (self.0)()
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Clock")
    }
}
