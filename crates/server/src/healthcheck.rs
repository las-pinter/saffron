//! A self-probe for container healthchecks.
//!
//! The runtime image has no shell or HTTP client, so the binary checks its own
//! `/api/health` endpoint with a minimal HTTP/1.1 request.

use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Requests `/api/health` from the server listening on `bind` and fails unless it
/// answers 200. A wildcard bind address is probed on loopback.
pub async fn probe(bind: SocketAddr) -> anyhow::Result<()> {
    let target = probe_target(bind);
    timeout(PROBE_TIMEOUT, request_health(target))
        .await
        .with_context(|| format!("no answer from {target} within {PROBE_TIMEOUT:?}"))?
}

fn probe_target(bind: SocketAddr) -> SocketAddr {
    let mut target = bind;
    if target.ip().is_unspecified() {
        target.set_ip(if bind.is_ipv4() {
            std::net::Ipv4Addr::LOCALHOST.into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        });
    }
    target
}

async fn request_health(target: SocketAddr) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect(target)
        .await
        .with_context(|| format!("cannot connect to {target}"))?;
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .context("cannot send the health request")?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .context("cannot read the health response")?;

    let status_line = response
        .split(|&byte| byte == b'\n')
        .next()
        .map(String::from_utf8_lossy)
        .unwrap_or_default();
    if status_code(&status_line) == Some("200") {
        Ok(())
    } else {
        bail!("unhealthy answer: {}", status_line.trim_end())
    }
}

/// The status code token of an HTTP status line (`HTTP/1.1 200 OK` → `200`).
fn status_code(status_line: &str) -> Option<&str> {
    status_line.split_whitespace().nth(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_reads_the_second_token() {
        assert_eq!(status_code("HTTP/1.1 200 OK\r"), Some("200"));
    }

    #[test]
    fn status_code_does_not_match_a_longer_code() {
        assert_ne!(status_code("HTTP/1.1 2000 Odd\r"), Some("200"));
    }

    #[test]
    fn probe_target_uses_loopback_for_wildcard_ipv4() {
        let target = probe_target("0.0.0.0:8640".parse().expect("valid address"));

        assert_eq!(target, "127.0.0.1:8640".parse().expect("valid address"));
    }

    #[test]
    fn probe_target_uses_loopback_for_wildcard_ipv6() {
        let target = probe_target("[::]:8640".parse().expect("valid address"));

        assert_eq!(target, "[::1]:8640".parse().expect("valid address"));
    }

    #[test]
    fn probe_target_keeps_a_specific_address() {
        let target = probe_target("100.64.0.7:8640".parse().expect("valid address"));

        assert_eq!(target, "100.64.0.7:8640".parse().expect("valid address"));
    }
}
