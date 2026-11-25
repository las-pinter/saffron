//! HTTP-level tests of the router.

use std::path::Path;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

struct Reply {
    status: StatusCode,
    content_type: Option<String>,
    body: Vec<u8>,
}

async fn send(method: Method, path: &str) -> Reply {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("valid request");
    let response = saffron_server::app()
        .oneshot(request)
        .await
        .expect("router answers");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body reads")
        .to_bytes()
        .to_vec();
    Reply {
        status,
        content_type,
        body,
    }
}

/// Debug builds read the bundle from disk, so the expected answer for web paths depends
/// on whether `just build` (or `pnpm build`) has produced `web/dist`.
fn bundle_built() -> bool {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/dist/index.html")
        .exists()
}

fn assert_serves_web_app(reply: &Reply) {
    let body = String::from_utf8_lossy(&reply.body);
    if bundle_built() {
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.content_type.as_deref(), Some("text/html"));
        assert!(body.contains(r#"<div id="root">"#), "body: {body}");
    } else {
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert!(body.contains("web UI not built"), "body: {body}");
    }
}

fn assert_api_not_found(reply: &Reply) {
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body, Vec::<u8>::new());
}

#[tokio::test]
async fn health_returns_ok_with_name_and_version() {
    let reply = send(Method::GET, "/api/health").await;

    assert_eq!(reply.status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&reply.body).expect("health is JSON");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["name"], "saffron");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn unknown_api_path_returns_404_not_the_web_app() {
    let reply = send(Method::GET, "/api/does-not-exist").await;

    assert_api_not_found(&reply);
}

#[tokio::test]
async fn api_root_returns_404_not_the_web_app() {
    let reply = send(Method::GET, "/api").await;

    assert_api_not_found(&reply);
}

#[tokio::test]
async fn api_root_with_slash_returns_404_not_the_web_app() {
    let reply = send(Method::GET, "/api/").await;

    assert_api_not_found(&reply);
}

#[tokio::test]
async fn root_serves_the_web_app() {
    let reply = send(Method::GET, "/").await;

    assert_serves_web_app(&reply);
}

#[tokio::test]
async fn client_route_serves_the_web_app() {
    let reply = send(Method::GET, "/holdings/nokia").await;

    assert_serves_web_app(&reply);
}

#[tokio::test]
async fn missing_asset_returns_404_not_the_web_app() {
    let reply = send(Method::GET, "/assets/missing-1a2b.js").await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_ne!(reply.content_type.as_deref(), Some("text/html"));
}

#[tokio::test]
async fn non_get_request_to_web_path_returns_405() {
    let reply = send(Method::POST, "/holdings").await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
}
