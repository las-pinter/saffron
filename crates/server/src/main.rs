//! The `saffron` binary.

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Context;
use saffron_server::{
    AppState,
    auth::{client::parse_trusted_proxies, origin::PublicOrigin},
    clock::Clock,
};
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

    let origin = PublicOrigin::from_env().context("SAFFRON_PUBLIC_URL is not usable")?;
    let trusted_proxies =
        parse_trusted_proxies(&std::env::var("SAFFRON_TRUSTED_PROXIES").unwrap_or_default())
            .map_err(anyhow::Error::msg)
            .context("SAFFRON_TRUSTED_PROXIES is not usable")?;

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

    let state =
        AppState::new(pool.clone(), origin, Clock::system()).with_trusted_proxies(trusted_proxies);
    announce_setup(&state).await?;
    tracing::info!(%addr, origin = %state.origin.as_str(), "saffron listening");

    let app = saffron_server::app(state).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server stopped with an error");
    // Closing checkpoints the WAL into the database file.
    pool.close().await;
    served
}

/// While no user exists, logs a one-time link that creates the first administrator.
async fn announce_setup(state: &AppState) -> anyhow::Result<()> {
    if saffron_db::users::count(&state.pool).await? > 0 {
        return Ok(());
    }
    let token = state.setup.issue()?;
    // The token rides in the fragment, which browsers never send to a server or put
    // in a Referer header.
    tracing::warn!(
        "no users yet; create the administrator at {}/setup#token={token}",
        state.origin.as_str()
    );
    Ok(())
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
