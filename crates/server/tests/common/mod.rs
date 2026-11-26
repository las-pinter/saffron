//! A test server over an in-memory database, with a clock the test controls.

// Each test file uses a different subset of these helpers.
#![allow(dead_code)]

pub mod authenticator;
pub mod instrument_steps;
pub mod passkey_steps;
pub mod portfolio_steps;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use jiff::{SignedDuration, Timestamp};
use saffron_server::{AppState, auth::origin::PublicOrigin, clock::Clock};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const LOCAL_ORIGIN: &str = "http://localhost:8640";
/// Where no Yahoo answers, for tests that do not look instruments up.
pub const NO_YAHOO: &str = "http://127.0.0.1:1";
pub const ADMIN_PASSWORD: &str = "admin password 1";
pub const USER_PASSWORD: &str = "member password 1";

/// A router with its state and a manual clock.
#[derive(Clone)]
pub struct TestApp {
    pub state: AppState,
    router: Router,
    now: Arc<Mutex<Timestamp>>,
}

impl TestApp {
    pub async fn new() -> Self {
        Self::with_origin(LOCAL_ORIGIN).await
    }

    pub async fn with_origin(origin: &str) -> Self {
        Self::build(origin, "", NO_YAHOO).await
    }

    pub async fn with_trusted_proxies(proxies: &str) -> Self {
        Self::build(LOCAL_ORIGIN, proxies, NO_YAHOO).await
    }

    /// An app that looks instruments up at `yahoo_url`.
    pub async fn with_yahoo(yahoo_url: &str) -> Self {
        Self::build(LOCAL_ORIGIN, "", yahoo_url).await
    }

    async fn build(origin: &str, proxies: &str, yahoo_url: &str) -> Self {
        let pool = saffron_db::open_in_memory().await.expect("database");
        let now = Arc::new(Mutex::new(
            "2026-11-25T12:00:00Z"
                .parse::<Timestamp>()
                .expect("timestamp"),
        ));
        let clock_now = Arc::clone(&now);
        let clock =
            Clock::from_fn(move || *clock_now.lock().unwrap_or_else(PoisonError::into_inner));
        let origin = PublicOrigin::parse(origin).expect("origin");
        let proxies =
            saffron_server::auth::client::parse_trusted_proxies(proxies).expect("trusted proxies");
        let secrets = saffron_server::secrets::SecretBox::random().expect("secret key");
        let yahoo = saffron_market::yahoo::Yahoo::new(yahoo_url, std::time::Duration::ZERO)
            .expect("Yahoo client");
        let state =
            AppState::new(pool, origin, clock, secrets, yahoo).with_trusted_proxies(proxies);
        Self {
            router: saffron_server::app(state.clone()),
            state,
            now,
        }
    }

    /// Serves `extra` routes beside the application, for probes of shared layers.
    pub fn with_routes(mut self, extra: Router) -> Self {
        self.router = extra.merge(saffron_server::app(self.state.clone()));
        self
    }

    pub fn advance(&self, by: SignedDuration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now += by;
    }

    pub fn get(&self, path: &str) -> Call<'_> {
        self.call(Method::GET, path)
    }

    pub fn post(&self, path: &str) -> Call<'_> {
        self.call(Method::POST, path)
    }

    pub fn call(&self, method: Method, path: &str) -> Call<'_> {
        Call {
            app: self,
            method,
            path: path.to_owned(),
            body: None,
            cookie: None,
            origin: Some(self.state.origin.as_str()),
            peer: None,
            extra: Vec::new(),
            raw: None,
        }
    }

    /// Creates the administrator `admin` through the setup endpoint; returns the
    /// session cookie.
    pub async fn setup_admin(&self) -> String {
        let token = self.state.setup.issue().expect("setup token");
        let reply = self
            .post("/api/auth/setup")
            .json(json!({
                "token": token,
                "username": "admin",
                "display_name": "Admin",
                "password": ADMIN_PASSWORD,
            }))
            .send()
            .await;
        assert_eq!(
            reply.status,
            StatusCode::CREATED,
            "setup: {:?}",
            reply.json()
        );
        reply.session_cookie().expect("setup sets a cookie")
    }

    /// Creates a regular user with [`USER_PASSWORD`]; returns its id.
    pub async fn create_user(&self, admin_cookie: &str, username: &str) -> i64 {
        let reply = self
            .post("/api/users")
            .cookie(admin_cookie)
            .json(json!({
                "username": username,
                "display_name": username,
                "password": USER_PASSWORD,
            }))
            .send()
            .await;
        assert_eq!(
            reply.status,
            StatusCode::CREATED,
            "create: {:?}",
            reply.json()
        );
        reply.json()["id"].as_i64().expect("user id")
    }

    pub async fn login(&self, username: &str, password: &str) -> Reply {
        self.post("/api/auth/login")
            .json(json!({ "username": username, "password": password }))
            .send()
            .await
    }
}

/// One request under construction.
pub struct Call<'a> {
    app: &'a TestApp,
    method: Method,
    path: String,
    body: Option<Value>,
    cookie: Option<String>,
    origin: Option<String>,
    peer: Option<SocketAddr>,
    extra: Vec<(&'static str, String)>,
    raw: Option<String>,
}

impl Call<'_> {
    pub fn header(mut self, name: &'static str, value: &str) -> Self {
        self.extra.push((name, value.to_owned()));
        self
    }

    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub fn raw_json(mut self, body: &str) -> Self {
        self.raw = Some(body.to_owned());
        self
    }

    pub fn cookie(mut self, cookie: &str) -> Self {
        self.cookie = Some(cookie.to_owned());
        self
    }

    pub fn origin(mut self, origin: Option<&str>) -> Self {
        self.origin = origin.map(str::to_owned);
        self
    }

    pub fn peer(mut self, peer: &str) -> Self {
        self.peer = Some(peer.parse().expect("socket address"));
        self
    }

    pub async fn send(self) -> Reply {
        let mut request = Request::builder().method(self.method).uri(&self.path);
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(origin) = &self.origin {
            request = request.header(header::ORIGIN, origin);
        }
        for (name, value) in &self.extra {
            request = request.header(*name, value);
        }
        let text = self
            .raw
            .clone()
            .or_else(|| self.body.as_ref().map(Value::to_string));
        let body = match text {
            Some(text) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(text)
            }
            None => Body::empty(),
        };
        let mut request = request.body(body).expect("valid request");
        if let Some(peer) = self.peer {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        let response = self
            .app
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router answers");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body reads")
            .to_bytes()
            .to_vec();
        Reply {
            status,
            headers,
            body,
        }
    }
}

/// A finished response.
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The `name=value` pair of the `Set-Cookie` header, ready for a `Cookie` header.
    pub fn session_cookie(&self) -> Option<String> {
        self.header("set-cookie")
            .and_then(|value| value.split(';').next())
            .map(str::to_owned)
    }
}
