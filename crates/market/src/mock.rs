//! A local stand-in for Yahoo that serves recorded responses, for tests.
//!
//! The fixtures were recorded from the live endpoints on 2026-11-26 and trimmed
//! to the fields Saffron reads. Like Yahoo, it answers 429 to a request without a
//! browser-like `User-Agent`.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use tokio::{net::TcpListener, task::JoinHandle};

use crate::{MarketError, yahoo::Yahoo};

const SEARCHES: &[(&str, &str)] = &[
    (
        "fi4000315700",
        include_str!("../fixtures/yahoo/search_FI4000315700.json"),
    ),
    (
        "ie00b4l5y983",
        include_str!("../fixtures/yahoo/search_IE00B4L5Y983.json"),
    ),
    ("nokia", include_str!("../fixtures/yahoo/search_nokia.json")),
    (
        "omxh25",
        include_str!("../fixtures/yahoo/search_omxh25.json"),
    ),
];

const CHARTS: &[(&str, &str)] = &[
    (
        "0P0001DHRJ.F",
        include_str!("../fixtures/yahoo/chart_0P0001DHRJ.F.json"),
    ),
    (
        "IWDA.L",
        include_str!("../fixtures/yahoo/chart_IWDA.L.json"),
    ),
    ("VOD.L", include_str!("../fixtures/yahoo/chart_VOD.L.json")),
    (
        "^OMXH25",
        include_str!("../fixtures/yahoo/chart_OMXH25.json"),
    ),
    (
        "NOKIA.HE",
        include_str!("../fixtures/yahoo/chart_NOKIA.HE.json"),
    ),
];

const CHART_NOT_FOUND: &str = include_str!("../fixtures/yahoo/chart_not_found.json");
const NO_RESULTS: &str = r#"{"count":0,"quotes":[],"news":[]}"#;

/// A running stand-in. It stops when dropped.
#[derive(Debug)]
pub struct MockYahoo {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

#[derive(Debug, Default)]
struct Shared {
    requests: AtomicUsize,
    forced_status: AtomicU16,
}

impl MockYahoo {
    /// Starts a stand-in on a free local port.
    ///
    /// # Panics
    ///
    /// When no local port can be bound.
    pub async fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let app = Router::new()
            .route("/v1/finance/search", get(search))
            .route("/v8/finance/chart/{symbol}", get(chart))
            .with_state(Arc::clone(&shared));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a local port");
        let addr = listener.local_addr().expect("local address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        Self { addr, shared, task }
    }

    /// The base URL to give [`Yahoo::new`].
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// A client for this stand-in with no gap between requests.
    pub fn client(&self) -> Result<Yahoo, MarketError> {
        Yahoo::new(&self.url(), Duration::ZERO)
    }

    /// Answers every later request with `status` and an empty body.
    pub fn fail_with(&self, status: StatusCode) {
        self.shared
            .forced_status
            .store(status.as_u16(), Ordering::SeqCst);
    }

    /// How many requests it has answered.
    #[must_use]
    pub fn requests(&self) -> usize {
        self.shared.requests.load(Ordering::SeqCst)
    }
}

impl Drop for MockYahoo {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Response {
    if let Some(refusal) = refusal(&shared, &headers) {
        return refusal;
    }
    let key = query.q.trim().to_lowercase();
    let body = SEARCHES
        .iter()
        .find(|(q, _)| *q == key)
        .map_or(NO_RESULTS, |(_, body)| body);
    json(StatusCode::OK, body)
}

async fn chart(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Path(symbol): Path<String>,
) -> Response {
    if let Some(refusal) = refusal(&shared, &headers) {
        return refusal;
    }
    // Yahoo matches symbols without regard to case.
    match CHARTS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(&symbol))
    {
        Some((_, body)) => json(StatusCode::OK, body),
        None => json(StatusCode::NOT_FOUND, CHART_NOT_FOUND),
    }
}

fn refusal(shared: &Shared, headers: &HeaderMap) -> Option<Response> {
    shared.requests.fetch_add(1, Ordering::SeqCst);
    let forced = shared.forced_status.load(Ordering::SeqCst);
    if forced != 0 {
        let status = StatusCode::from_u16(forced).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return Some(status.into_response());
    }
    let browser_like = headers
        .get(header::USER_AGENT)
        .and_then(|agent| agent.to_str().ok())
        .is_some_and(|agent| agent.starts_with("Mozilla/"));
    (!browser_like)
        .then(|| (StatusCode::TOO_MANY_REQUESTS, "Edge: Too Many Requests").into_response())
}

fn json(status: StatusCode, body: &'static str) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}
