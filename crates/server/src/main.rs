//! The `saffron` binary.

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Context;
use tracing_subscriber::EnvFilter;

/// Loopback by default: Saffron is meant to sit behind a VPN or reverse proxy.
const DEFAULT_BIND: &str = "127.0.0.1:8640";

/// Relative to the working directory; the container image sets `/data`.
const DEFAULT_DATA_DIR: &str = "data";

const USAGE: &str = "usage: saffron [healthcheck]";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => serve(bind_address()?).await,
        [command] if command == "healthcheck" => {
            saffron_server::healthcheck::probe(bind_address()?).await
        }
        _ => anyhow::bail!("unexpected arguments {args:?}; {USAGE}"),
    }
}

fn bind_address() -> anyhow::Result<SocketAddr> {
    let bind = std::env::var("SAFFRON_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_owned());
    bind.parse()
        .with_context(|| format!("SAFFRON_BIND is not a socket address: {bind}"))
}

async fn serve(addr: SocketAddr) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Bind first: a busy port then fails before the database is opened.
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr}"))?;

    let data_dir = PathBuf::from(
        std::env::var_os("SAFFRON_DATA_DIR")
            .filter(|dir| !dir.is_empty())
            .unwrap_or_else(|| DEFAULT_DATA_DIR.into()),
    );
    let database = data_dir.join(saffron_db::DATABASE_FILE);
    let pool = saffron_db::open(&database)
        .await
        .with_context(|| format!("cannot open the database {}", database.display()))?;
    tracing::info!(database = %database.display(), "database ready");
    tracing::info!(%addr, "saffron listening");

    let served = axum::serve(listener, saffron_server::app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server stopped with an error");
    // Closing checkpoints the WAL into the database file.
    pool.close().await;
    served
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
