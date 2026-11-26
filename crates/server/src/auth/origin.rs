//! The public origin: the URL the browser uses to reach Saffron.
//!
//! Saffron listens on loopback and is reached through Tailscale or a reverse proxy,
//! so the browser's origin differs from the listen address. The origin decides the
//! cookie flags, the request-origin check, and the passkey relying party.

use thiserror::Error;
use url::{Host, Url};

/// The origin when `SAFFRON_PUBLIC_URL` is unset: the default listen address.
pub const DEFAULT_PUBLIC_URL: &str = "http://localhost:8640";

/// A validated public origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOrigin {
    url: Url,
}

/// Why an origin was rejected.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid public URL `{value}`: {reason}")]
pub struct OriginError {
    /// The offending value.
    pub value: String,
    /// Why it was rejected.
    pub reason: &'static str,
}

impl PublicOrigin {
    /// Parses and validates an origin such as `https://saffron.example.ts.net`.
    pub fn parse(raw: &str) -> Result<Self, OriginError> {
        let invalid = |reason| OriginError {
            value: raw.to_owned(),
            reason,
        };
        let url = Url::parse(raw.trim()).map_err(|_| invalid("not an absolute URL"))?;
        if url.host_str().is_none() {
            return Err(invalid("a host is required"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid("credentials are not part of an origin"));
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(invalid("an origin has no path, query, or fragment"));
        }
        match url.scheme() {
            "https" => {}
            "http" if matches!(url.host(), Some(Host::Domain("localhost"))) => {}
            "http" => return Err(invalid("plain HTTP is allowed only for http://localhost")),
            _ => return Err(invalid("the scheme must be https or http")),
        }
        Ok(Self { url })
    }

    /// The origin from `SAFFRON_PUBLIC_URL`, or [`DEFAULT_PUBLIC_URL`] when unset or
    /// empty.
    pub fn from_env() -> Result<Self, OriginError> {
        let value = std::env::var("SAFFRON_PUBLIC_URL").unwrap_or_default();
        let value = value.trim();
        Self::parse(if value.is_empty() {
            DEFAULT_PUBLIC_URL
        } else {
            value
        })
    }

    /// The origin as a browser sends it (`scheme://host[:port]`).
    #[must_use]
    pub fn as_str(&self) -> String {
        serialize(&self.url)
    }

    /// Whether the browser reaches Saffron over HTTPS.
    #[must_use]
    pub fn is_https(&self) -> bool {
        self.url.scheme() == "https"
    }

    /// Whether a browser `Origin` header belongs to this deployment.
    ///
    /// On the localhost origin any loopback port is accepted, so the Vite dev server
    /// can call the API; only local processes reach a loopback listener anyway.
    #[must_use]
    pub fn accepts(&self, candidate: &str) -> bool {
        let Ok(url) = Url::parse(candidate) else {
            return false;
        };
        serialize(&url) == self.as_str()
            || (!self.is_https() && url.scheme() == "http" && is_loopback(&url))
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    }
}

fn serialize(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(raw: &str) -> PublicOrigin {
        PublicOrigin::parse(raw).expect("valid origin")
    }

    #[test]
    fn https_origin_is_accepted() {
        assert!(origin("https://saffron.example.ts.net").is_https());
    }

    #[test]
    fn localhost_http_origin_is_accepted() {
        assert!(!origin("http://localhost:8640").is_https());
    }

    #[test]
    fn plain_http_on_another_host_is_rejected() {
        let error = PublicOrigin::parse("http://saffron.lan").expect_err("not https");

        assert_eq!(
            error.reason,
            "plain HTTP is allowed only for http://localhost"
        );
    }

    #[test]
    fn an_origin_with_a_path_is_rejected() {
        assert!(PublicOrigin::parse("https://example.test/saffron").is_err());
    }

    #[test]
    fn an_origin_with_credentials_is_rejected() {
        assert!(PublicOrigin::parse("https://user:pw@example.test").is_err());
    }

    #[test]
    fn another_scheme_is_rejected() {
        assert!(PublicOrigin::parse("ftp://example.test").is_err());
    }

    #[test]
    fn a_relative_value_is_rejected() {
        assert!(PublicOrigin::parse("example.test").is_err());
    }

    #[test]
    fn as_str_drops_the_trailing_slash_and_keeps_the_port() {
        assert_eq!(
            origin("https://example.test:8443/").as_str(),
            "https://example.test:8443"
        );
    }

    #[test]
    fn https_accepts_only_its_own_origin() {
        let public = origin("https://example.test");

        assert!(public.accepts("https://example.test"));
        assert!(!public.accepts("https://evil.test"));
        assert!(!public.accepts("http://localhost:5173"));
        assert!(!public.accepts("null"));
    }

    #[test]
    fn localhost_accepts_other_loopback_ports() {
        let public = origin("http://localhost:8640");

        assert!(public.accepts("http://localhost:5173"));
        assert!(public.accepts("http://127.0.0.1:5173"));
        assert!(!public.accepts("http://example.test"));
    }
}
