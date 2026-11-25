//! The `saffron` binary.

use std::net::SocketAddr;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

/// Loopback by default: Saffron is meant to sit behind a VPN or reverse proxy.
const DEFAULT_BIND: &str = "127.0.0.1:8640";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind = std::env::var("SAFFRON_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_owned());
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("SAFFRON_BIND is not a socket address: {bind}"))?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr}"))?;
    tracing::info!(%addr, "saffron listening");

    axum::serve(listener, saffron_server::app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server stopped with an error")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "cannot listen for Ctrl-C");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "cannot listen for SIGTERM"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
