//! Steps for instrument tests: an app wired to a stand-in Yahoo, and its calls.

use saffron_market::mock::MockMarket;
use serde_json::Value;

use super::{Reply, TestApp};

/// An app whose Yahoo is a local stand-in, with the administrator signed in.
pub struct Market {
    pub app: TestApp,
    pub yahoo: MockMarket,
    pub admin: String,
}

pub async fn market() -> Market {
    let yahoo = MockMarket::start().await;
    let app = TestApp::with_yahoo(&yahoo.url()).await;
    let admin = app.setup_admin().await;
    Market { app, yahoo, admin }
}

pub async fn lookup(market: &Market, query: &str) -> Reply {
    let query: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    market
        .app
        .get(&format!("/api/instruments/lookup?q={query}"))
        .cookie(&market.admin)
        .send()
        .await
}

pub async fn add(market: &Market, body: Value) -> Reply {
    market
        .app
        .post("/api/instruments")
        .cookie(&market.admin)
        .json(body)
        .send()
        .await
}

pub async fn instrument_count(market: &Market) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM instruments")
        .fetch_one(&market.app.state.pool)
        .await
        .expect("count")
}

/// Signs in the member user and returns their session cookie.
pub async fn member_cookie(market: &Market) -> String {
    market
        .app
        .login("member", super::USER_PASSWORD)
        .await
        .session_cookie()
        .expect("cookie")
}
