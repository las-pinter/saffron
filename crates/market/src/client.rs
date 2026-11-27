//! The HTTP side shared by every provider: one client with a browser-like
//! `User-Agent`, a timeout, and a [`Throttle`] that spaces requests and backs off
//! after the provider refuses with 429.

use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::{sync::Mutex, time::Instant};
use url::Url;

use crate::MarketError;

/// Most requests that may wait for their turn; more are refused as
/// [`MarketError::Busy`], so one burst cannot park every caller.
pub const MAX_WAITING: u32 = 4;
/// The first pause after a 429; each further 429 doubles it.
pub const FIRST_BACKOFF: Duration = Duration::from_secs(30);
/// The longest pause after repeated 429s.
pub const MAX_BACKOFF: Duration = Duration::from_hours(1);

// Yahoo answers 429 to clients without a browser-like agent.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
const TIMEOUT: Duration = Duration::from_secs(10);

/// An HTTP client for one provider.
#[derive(Debug)]
pub(crate) struct Client {
    http: reqwest::Client,
    base: Url,
    throttle: Throttle,
}

impl Client {
    /// A client for the endpoints at `base`, spacing requests `gap` apart.
    pub(crate) fn new(base: &str, gap: Duration) -> Result<Self, MarketError> {
        // reqwest is built without a TLS provider so that aws-lc stays out of the
        // build; ring is installed here. A second install fails harmlessly.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let base = Url::parse(base).map_err(|error| {
            MarketError::Unavailable(format!("bad provider URL {base}: {error}"))
        })?;
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .map_err(|error| MarketError::Unavailable(error.to_string()))?;
        Ok(Self {
            http,
            base,
            throttle: Throttle::new(gap),
        })
    }

    /// The base URL with `segments` appended, each escaped as one path segment.
    pub(crate) fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("an http URL has path segments")
            .pop_if_empty()
            .extend(segments);
        url
    }

    /// Fetches `url` in its turn and reads the body as JSON.
    pub(crate) async fn get_json<T: DeserializeOwned>(&self, url: Url) -> Result<T, MarketError> {
        let body = self.get_bytes(url).await?;
        serde_json::from_slice(&body).map_err(|error| MarketError::BadResponse(error.to_string()))
    }

    /// Fetches `url` in its turn and reads the body as bytes.
    pub(crate) async fn get_bytes(&self, url: Url) -> Result<Vec<u8>, MarketError> {
        self.throttle.wait().await?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| MarketError::Unavailable(error.without_url().to_string()))?;
        match response.status().as_u16() {
            200..=299 => self.throttle.succeeded().await,
            404 => return Err(MarketError::NotFound),
            429 => {
                self.throttle.refused().await;
                return Err(MarketError::RateLimited);
            }
            status => return Err(MarketError::Unavailable(format!("HTTP {status}"))),
        }
        response
            .bytes()
            .await
            .map(|body| body.to_vec())
            .map_err(|error| MarketError::Unavailable(error.without_url().to_string()))
    }
}

/// Spaces requests at least `gap` apart, bounds how many may wait, and pauses
/// every request after the provider answers 429.
#[derive(Debug)]
pub(crate) struct Throttle {
    gap: Duration,
    state: Mutex<ThrottleState>,
}

#[derive(Debug)]
struct ThrottleState {
    next_slot: Instant,
    paused_until: Option<Instant>,
    backoff: Duration,
}

impl Throttle {
    pub(crate) fn new(gap: Duration) -> Self {
        Self {
            gap,
            state: Mutex::new(ThrottleState {
                next_slot: Instant::now(),
                paused_until: None,
                backoff: Duration::ZERO,
            }),
        }
    }

    /// Waits for the next free slot. Refuses at once with
    /// [`MarketError::RateLimited`] during a pause after a 429, and with
    /// [`MarketError::Busy`] when too many requests already wait.
    pub(crate) async fn wait(&self) -> Result<(), MarketError> {
        let start = {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            if state.paused_until.is_some_and(|until| now < until) {
                return Err(MarketError::RateLimited);
            }
            let start = state.next_slot.max(now);
            if start > now + self.gap * MAX_WAITING {
                return Err(MarketError::Busy);
            }
            state.next_slot = start + self.gap;
            start
        };
        tokio::time::sleep_until(start).await;
        Ok(())
    }

    /// Records a 429: pauses requests for twice the previous pause, starting at
    /// [`FIRST_BACKOFF`] and capped at [`MAX_BACKOFF`].
    pub(crate) async fn refused(&self) {
        let mut state = self.state.lock().await;
        state.backoff = (state.backoff * 2).clamp(FIRST_BACKOFF, MAX_BACKOFF);
        state.paused_until = Some(Instant::now() + state.backoff);
    }

    /// Records an answer: the next 429 starts the pause from the beginning.
    pub(crate) async fn succeeded(&self) {
        let mut state = self.state.lock().await;
        state.backoff = Duration::ZERO;
        state.paused_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_429_pauses_requests_for_the_first_backoff() {
        let throttle = Throttle::new(Duration::ZERO);
        throttle.refused().await;

        let during = throttle.wait().await;
        tokio::time::advance(FIRST_BACKOFF).await;
        let after = throttle.wait().await;

        assert!(
            matches!((during, after), (Err(MarketError::RateLimited), Ok(()))),
            "the pause lasts the first backoff"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_429_doubles_the_pause() {
        let throttle = Throttle::new(Duration::ZERO);
        throttle.refused().await;
        tokio::time::advance(FIRST_BACKOFF).await;
        throttle.refused().await;

        tokio::time::advance(FIRST_BACKOFF).await;
        let halfway = throttle.wait().await;
        tokio::time::advance(FIRST_BACKOFF).await;
        let after = throttle.wait().await;

        assert!(
            matches!((halfway, after), (Err(MarketError::RateLimited), Ok(()))),
            "the second pause is twice the first"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_pause_never_exceeds_the_maximum() {
        let throttle = Throttle::new(Duration::ZERO);
        for _ in 0..12 {
            throttle.refused().await;
        }

        tokio::time::advance(MAX_BACKOFF).await;

        assert!(throttle.wait().await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn an_answer_resets_the_backoff() {
        let throttle = Throttle::new(Duration::ZERO);
        throttle.refused().await;
        tokio::time::advance(FIRST_BACKOFF).await;
        throttle.succeeded().await;
        throttle.refused().await;

        tokio::time::advance(FIRST_BACKOFF).await;

        assert!(throttle.wait().await.is_ok());
    }
}
