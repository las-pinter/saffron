//! Sign-in throttling over HTTP: account lockout, the per-client brake, trusted
//! proxies, and the sign-in queue.

mod common;

use axum::http::StatusCode;
use common::{ADMIN_PASSWORD, TestApp};
use jiff::SignedDuration;
use serde_json::json;

#[tokio::test]
async fn five_failed_logins_lock_the_account_even_for_the_right_password() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for _ in 0..5 {
        app.login("admin", "wrong password 1").await;
    }

    let reply = app.login("admin", ADMIN_PASSWORD).await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::TOO_MANY_REQUESTS, json!("account_locked"))
    );
    assert_eq!(reply.header("retry-after"), Some("900"));
}

#[tokio::test]
async fn a_locked_account_opens_again_after_fifteen_minutes() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for _ in 0..5 {
        app.login("admin", "wrong password 1").await;
    }

    app.advance(SignedDuration::from_mins(15));
    let reply = app.login("admin", ADMIN_PASSWORD).await;

    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn failures_on_many_usernames_throttle_the_client() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for attempt in 0..20 {
        app.post("/api/auth/login")
            .peer("10.0.0.9:50000")
            .json(json!({ "username": format!("guess{attempt}"), "password": "x" }))
            .send()
            .await;
    }

    let reply = app
        .post("/api/auth/login")
        .peer("10.0.0.9:50001")
        .json(json!({ "username": "admin", "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::TOO_MANY_REQUESTS, json!("too_many_attempts"))
    );
}

#[tokio::test]
async fn another_client_is_not_throttled_by_failures_elsewhere() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for attempt in 0..20 {
        app.post("/api/auth/login")
            .peer("10.0.0.9:50000")
            .json(json!({ "username": format!("guess{attempt}"), "password": "x" }))
            .send()
            .await;
    }

    let reply = app
        .post("/api/auth/login")
        .peer("10.0.0.10:50000")
        .json(json!({ "username": "admin", "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn parallel_guesses_cannot_overshoot_the_lockout() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    let mut guesses = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let app = app.clone();
        guesses.spawn(async move { app.login("admin", "wrong password 1").await.status });
    }

    let statuses = guesses.join_all().await;

    let checked = statuses
        .iter()
        .filter(|status| **status == StatusCode::UNAUTHORIZED)
        .count();
    assert_eq!(checked, 5, "statuses: {statuses:?}");
}

#[tokio::test]
async fn a_trusted_proxy_tells_clients_apart() {
    let app = TestApp::with_trusted_proxies("172.16.0.0/12").await;
    app.setup_admin().await;
    for attempt in 0..20 {
        app.post("/api/auth/login")
            .peer("172.18.0.1:40000")
            .header("x-forwarded-for", "100.64.0.9")
            .json(json!({ "username": format!("guess{attempt}"), "password": "x" }))
            .send()
            .await;
    }

    let reply = app
        .post("/api/auth/login")
        .peer("172.18.0.1:40001")
        .header("x-forwarded-for", "100.64.0.10")
        .json(json!({ "username": "admin", "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn an_untrusted_peer_cannot_dodge_the_brake_with_a_forwarded_header() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for attempt in 0..20 {
        app.post("/api/auth/login")
            .peer("10.0.0.9:50000")
            .header("x-forwarded-for", &format!("100.64.0.{attempt}"))
            .json(json!({ "username": format!("guess{attempt}"), "password": "x" }))
            .send()
            .await;
    }

    let reply = app
        .post("/api/auth/login")
        .peer("10.0.0.9:50001")
        .header("x-forwarded-for", "100.64.0.99")
        .json(json!({ "username": "admin", "password": ADMIN_PASSWORD }))
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_full_sign_in_queue_refuses_more_requests() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    let gate = app.state.sign_in_gate.lock().await;
    let mut waiting = tokio::task::JoinSet::new();
    for _ in 0..saffron_server::state::MAX_QUEUED_SIGN_INS {
        let app = app.clone();
        waiting.spawn(async move { app.login("admin", ADMIN_PASSWORD).await.status });
    }
    // Bounded, so a queue larger than the limit fails the test instead of hanging.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while app.state.sign_in_queue.available_permits() > 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;

    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.login("admin", ADMIN_PASSWORD),
    )
    .await
    .expect("refused without waiting for the gate");

    drop(gate);
    waiting.join_all().await;
    assert_eq!(
        (reply.status, reply.json()["error"].clone()),
        (StatusCode::TOO_MANY_REQUESTS, json!("sign_in_busy"))
    );
}

#[tokio::test]
async fn a_throttled_client_is_refused_without_waiting_its_turn() {
    let app = TestApp::new().await;
    app.setup_admin().await;
    for attempt in 0..20 {
        saffron_db::login_throttle::record_failure(
            &app.state.pool,
            &format!("guess{attempt}"),
            "10.0.0.9",
            app.state.clock.now(),
        )
        .await
        .expect("record");
    }
    let _gate = app.state.sign_in_gate.lock().await;

    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.post("/api/auth/login")
            .peer("10.0.0.9:50000")
            .json(json!({ "username": "admin", "password": ADMIN_PASSWORD }))
            .send(),
    )
    .await
    .expect("answered without waiting for the gate");

    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_huge_username_is_recorded_cut_short() {
    let app = TestApp::new().await;
    app.setup_admin().await;

    app.login(&"x".repeat(10_000), "wrong password 1").await;

    let length: i64 = sqlx::query_scalar("SELECT length(username) FROM login_failures")
        .fetch_one(&app.state.pool)
        .await
        .expect("failure row");
    assert_eq!(length, 65);
}

#[tokio::test]
async fn a_forged_first_forwarded_line_cannot_dodge_the_brake() {
    let app = TestApp::with_trusted_proxies("172.16.0.0/12").await;
    app.setup_admin().await;
    let attempt_from = |forged: String, username: String, password: &'static str| {
        app.post("/api/auth/login")
            .peer("172.18.0.1:40000")
            // The client's own line first; the proxy appends the real address.
            .header("x-forwarded-for", &forged)
            .header("x-forwarded-for", "100.64.0.9")
            .json(json!({ "username": username, "password": password }))
            .send()
    };
    for attempt in 0..20 {
        attempt_from(
            format!("198.51.100.{attempt}"),
            format!("guess{attempt}"),
            "x",
        )
        .await;
    }

    let reply = attempt_from(
        "198.51.100.99".to_owned(),
        "admin".to_owned(),
        ADMIN_PASSWORD,
    )
    .await;

    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
}
