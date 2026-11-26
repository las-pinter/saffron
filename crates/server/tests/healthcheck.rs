//! The container self-probe against real sockets.

use std::net::SocketAddr;

use saffron_server::healthcheck::probe;

mod common;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn local_listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds an ephemeral port");
    let addr = listener.local_addr().expect("has a local address");
    (listener, addr)
}

/// Accepts one connection and answers it with `status_line`.
async fn answer_once(listener: TcpListener, status_line: &'static str) {
    let (mut socket, _) = listener.accept().await.expect("accepts the probe");
    let mut request = [0_u8; 512];
    let _ = socket.read(&mut request).await.expect("reads the request");
    let response = format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    socket
        .write_all(response.as_bytes())
        .await
        .expect("writes the answer");
}

#[tokio::test]
async fn probe_succeeds_against_the_running_app() {
    let (listener, addr) = local_listener().await;
    let app = common::TestApp::new().await;
    let router = saffron_server::app(app.state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await });

    let result = probe(addr).await;

    assert!(result.is_ok(), "probe failed: {result:?}");
}

#[tokio::test]
async fn probe_fails_on_non_200_answer() {
    let (listener, addr) = local_listener().await;
    tokio::spawn(answer_once(listener, "HTTP/1.1 503 Service Unavailable"));

    let error = probe(addr).await.expect_err("503 is unhealthy");

    assert!(error.to_string().contains("503"), "error: {error}");
}

#[tokio::test]
async fn probe_fails_when_nothing_listens() {
    let (listener, addr) = local_listener().await;
    drop(listener);

    let error = probe(addr).await.expect_err("closed port is unhealthy");

    assert!(
        error.to_string().contains("cannot connect"),
        "error: {error}"
    );
}
