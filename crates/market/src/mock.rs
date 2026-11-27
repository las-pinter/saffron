//! A local stand-in for Yahoo and FT that serves recorded responses, for tests.
//!
//! The fixtures were recorded from the live endpoints on 2026-11-26 and trimmed
//! to the fields Saffron reads. Like Yahoo, it answers 429 to a request without a
//! browser-like `User-Agent`.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
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

use crate::{MarketError, Providers, ft::Ft, yahoo::Yahoo};

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
    // Built by hand: a fund whose provider price is a year old.
    (
        "STALE.F",
        include_str!("../fixtures/yahoo/chart_STALE.F.json"),
    ),
    (
        "NOKIA.HE",
        include_str!("../fixtures/yahoo/chart_NOKIA.HE.json"),
    ),
];

const FT_SEARCHES: &[(&str, &str)] = &[(
    "FI4000315700",
    include_str!("../fixtures/ft/search_FI4000315700.json"),
)];
/// FT sometimes lists one ISIN in several currencies; this answer puts USD first.
const FT_SEARCH_TWO_CURRENCIES: &str =
    include_str!("../fixtures/ft/search_FI4000315700_two_currencies.json");
const FT_SEARCH_FUZZY: &str = include_str!("../fixtures/ft/search_none.json");
const FT_HISTORIES: &[(&str, &str)] = &[
    (
        "118011809",
        include_str!("../fixtures/ft/history_118011809.json"),
    ),
    // The USD share class of the same fund, used by the currency-guard test.
    ("999999", include_str!("../fixtures/ft/history_999999.json")),
];
const FT_HISTORY_EMPTY: &str = include_str!("../fixtures/ft/history_empty.json");

const CHART_NOT_FOUND: &str = include_str!("../fixtures/yahoo/chart_not_found.json");
const NO_RESULTS: &str = r#"{"count":0,"quotes":[],"news":[]}"#;

/// A running stand-in. It stops when dropped.
#[derive(Debug)]
pub struct MockMarket {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

#[derive(Debug, Default)]
struct Shared {
    requests: AtomicUsize,
    spark_requests: AtomicUsize,
    forced_status: AtomicU16,
    forced_yahoo_status: AtomicU16,
    two_currency_ft: AtomicBool,
}

impl MockMarket {
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
            .route("/v8/finance/spark", get(spark))
            .route("/data/searchapi/searchsecurities", get(ft_search))
            .route("/data/equities/ajax/get-historical-prices", get(ft_history))
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

    /// Every provider pointed at this stand-in, with no gap between requests.
    pub fn providers(&self) -> Result<Providers, MarketError> {
        Ok(Providers {
            yahoo: self.client()?,
            ft: self.ft_client()?,
        })
    }

    /// An FT client for this stand-in with no gap between requests.
    pub fn ft_client(&self) -> Result<Ft, MarketError> {
        Ft::new(&self.url(), Duration::ZERO)
    }

    /// Answers every later request with `status` and an empty body.
    pub fn fail_with(&self, status: StatusCode) {
        self.shared
            .forced_status
            .store(status.as_u16(), Ordering::SeqCst);
    }

    /// Lists the OP fund's ISIN in USD before EUR, as FT does for share classes.
    pub fn serve_two_currency_funds(&self) {
        self.shared.two_currency_ft.store(true, Ordering::SeqCst);
    }

    /// Answers every later request normally again.
    pub fn recover(&self) {
        self.shared.forced_status.store(0, Ordering::SeqCst);
        self.shared.forced_yahoo_status.store(0, Ordering::SeqCst);
    }

    /// Answers every later Yahoo request with `status`, leaving FT alone.
    pub fn fail_yahoo_with(&self, status: StatusCode) {
        self.shared
            .forced_yahoo_status
            .store(status.as_u16(), Ordering::SeqCst);
    }

    /// How many batched quote requests it has answered.
    #[must_use]
    pub fn spark_requests(&self) -> usize {
        self.shared.spark_requests.load(Ordering::SeqCst)
    }

    /// How many requests it has answered.
    #[must_use]
    pub fn requests(&self) -> usize {
        self.shared.requests.load(Ordering::SeqCst)
    }
}

impl Drop for MockMarket {
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
    if let Some(refusal) = refusal_yahoo(&shared, &headers) {
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
    if let Some(refusal) = refusal_yahoo(&shared, &headers) {
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

#[derive(Debug, Deserialize)]
struct SparkQuery {
    symbols: String,
}

/// Answers the batched quote call with one series per chart fixture, in the
/// shape Yahoo uses: symbol to its timestamps and closes.
async fn spark(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(query): Query<SparkQuery>,
) -> Response {
    if let Some(refusal) = refusal_yahoo(&shared, &headers) {
        return refusal;
    }
    shared.spark_requests.fetch_add(1, Ordering::SeqCst);
    let mut series = serde_json::Map::new();
    for symbol in query.symbols.split(',') {
        let Some((_, body)) = CHARTS
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(symbol))
        else {
            continue;
        };
        let Ok(chart) = serde_json::from_str::<serde_json::Value>(body) else {
            continue;
        };
        let result = &chart["chart"]["result"][0];
        series.insert(
            symbol.to_owned(),
            serde_json::json!({
                "symbol": symbol,
                "timestamp": result["timestamp"],
                "close": result["indicators"]["quote"][0]["close"],
            }),
        );
    }
    json_owned(
        StatusCode::OK,
        serde_json::Value::Object(series).to_string(),
    )
}

#[derive(Debug, Deserialize)]
struct FtSearchQuery {
    query: String,
}

async fn ft_search(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(query): Query<FtSearchQuery>,
) -> Response {
    if let Some(refusal) = refusal(&shared, &headers) {
        return refusal;
    }
    // FT's search is fuzzy: an unknown ISIN still finds unrelated securities.
    let body = if shared.two_currency_ft.load(Ordering::SeqCst) {
        FT_SEARCH_TWO_CURRENCIES
    } else {
        FT_SEARCHES
            .iter()
            .find(|(isin, _)| *isin == query.query)
            .map_or(FT_SEARCH_FUZZY, |(_, body)| body)
    };
    json(StatusCode::OK, body)
}

#[derive(Debug, Deserialize)]
struct FtHistoryQuery {
    symbol: String,
}

async fn ft_history(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(query): Query<FtHistoryQuery>,
) -> Response {
    if let Some(refusal) = refusal(&shared, &headers) {
        return refusal;
    }
    let body = FT_HISTORIES
        .iter()
        .find(|(xid, _)| *xid == query.symbol)
        .map_or(FT_HISTORY_EMPTY, |(_, body)| body);
    json(StatusCode::OK, body)
}

fn refusal(shared: &Shared, headers: &HeaderMap) -> Option<Response> {
    refusal_with(shared, headers, shared.forced_status.load(Ordering::SeqCst))
}

/// Yahoo also obeys the switch that forces every provider to fail.
fn refusal_yahoo(shared: &Shared, headers: &HeaderMap) -> Option<Response> {
    let forced = shared.forced_yahoo_status.load(Ordering::SeqCst);
    let forced = match forced {
        0 => shared.forced_status.load(Ordering::SeqCst),
        forced => forced,
    };
    refusal_with(shared, headers, forced)
}

fn refusal_with(shared: &Shared, headers: &HeaderMap, forced: u16) -> Option<Response> {
    shared.requests.fetch_add(1, Ordering::SeqCst);
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

fn json_owned(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}
