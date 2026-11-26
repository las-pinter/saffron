//! The client address behind trusted proxies.
//!
//! Behind `tailscale serve`, a reverse proxy, or Docker's published port, every
//! request arrives from the proxy, so the peer address alone cannot tell clients
//! apart. `X-Forwarded-For` names the client, but any client can also send it, so
//! it is honored only when the peer is a proxy the operator listed in
//! `SAFFRON_TRUSTED_PROXIES`.

use std::net::IpAddr;

/// An address or CIDR range whose `X-Forwarded-For` header is believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedProxy {
    network: IpAddr,
    prefix: u8,
}

impl TrustedProxy {
    /// Parses `10.0.0.1`, `172.16.0.0/12`, or `fd00::/8`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let (address, prefix) = match raw.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (raw, None),
        };
        let network: IpAddr = address
            .parse()
            .map_err(|_| format!("`{raw}` is not an IP address or CIDR range"))?;
        let max = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) => prefix
                .parse::<u8>()
                .ok()
                .filter(|prefix| *prefix <= max)
                .ok_or_else(|| format!("`{raw}` has an invalid prefix length"))?,
            None => max,
        };
        Ok(Self { network, prefix })
    }

    /// Whether `address` lies in this range.
    #[must_use]
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                same_prefix(&network.octets(), &address.octets(), self.prefix)
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                same_prefix(&network.octets(), &address.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

/// Parses a comma-separated `SAFFRON_TRUSTED_PROXIES` value; empty means none.
pub fn parse_trusted_proxies(raw: &str) -> Result<Vec<TrustedProxy>, String> {
    raw.split(',')
        .filter(|part| !part.trim().is_empty())
        .map(TrustedProxy::parse)
        .collect()
}

/// The client address of a request from `peer` carrying `forwarded_for`.
///
/// Walks `X-Forwarded-For` from the right, past every trusted proxy, and takes the
/// first other address: entries further left were written by the client and can be
/// forged. An untrusted peer is the client itself.
#[must_use]
pub fn client_address(
    peer: Option<IpAddr>,
    forwarded_for: Option<&str>,
    trusted: &[TrustedProxy],
) -> String {
    let is_trusted = |address: IpAddr| trusted.iter().any(|proxy| proxy.contains(address));
    let Some(peer) = peer.map(|peer| peer.to_canonical()) else {
        return "unknown".to_owned();
    };
    if !is_trusted(peer) {
        return peer.to_string();
    }
    let forwarded = forwarded_for
        .unwrap_or_default()
        .rsplit(',')
        .map(|entry| entry.trim().parse::<IpAddr>().map(|ip| ip.to_canonical()));
    for entry in forwarded {
        match entry {
            Ok(address) if is_trusted(address) => {}
            Ok(address) => return address.to_string(),
            // A malformed hop: nothing to its left can be trusted.
            Err(_) => break,
        }
    }
    peer.to_string()
}

fn same_prefix(network: &[u8], address: &[u8], prefix: u8) -> bool {
    let full_bytes = usize::from(prefix / 8);
    let rest_bits = prefix % 8;
    if network[..full_bytes] != address[..full_bytes] {
        return false;
    }
    if rest_bits == 0 {
        return true;
    }
    let mask = 0xff_u8 << (8 - rest_bits);
    network[full_bytes] & mask == address[full_bytes] & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxies(raw: &str) -> Vec<TrustedProxy> {
        parse_trusted_proxies(raw).expect("valid proxies")
    }

    fn ip(raw: &str) -> Option<IpAddr> {
        raw.parse().ok()
    }

    #[test]
    fn a_single_address_matches_only_itself() {
        let proxy = TrustedProxy::parse("10.0.0.1").expect("proxy");

        assert!(proxy.contains("10.0.0.1".parse().expect("ip")));
        assert!(!proxy.contains("10.0.0.2".parse().expect("ip")));
    }

    #[test]
    fn a_cidr_range_matches_its_addresses() {
        let proxy = TrustedProxy::parse("172.16.0.0/12").expect("proxy");

        assert!(proxy.contains("172.31.255.1".parse().expect("ip")));
        assert!(!proxy.contains("172.32.0.1".parse().expect("ip")));
    }

    #[test]
    fn an_ipv6_range_matches_its_addresses() {
        let proxy = TrustedProxy::parse("fd00::/8").expect("proxy");

        assert!(proxy.contains("fd12::1".parse().expect("ip")));
        assert!(!proxy.contains("fe80::1".parse().expect("ip")));
    }

    #[test]
    fn an_ipv4_range_never_matches_ipv6() {
        let proxy = TrustedProxy::parse("0.0.0.0/0").expect("proxy");

        assert!(!proxy.contains("::1".parse().expect("ip")));
    }

    #[test]
    fn a_prefix_beyond_the_address_width_is_rejected() {
        assert!(TrustedProxy::parse("10.0.0.0/33").is_err());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_trusted_proxies("10.0.0.1, proxy.lan").is_err());
    }

    #[test]
    fn an_empty_setting_trusts_nothing() {
        assert_eq!(proxies(" "), []);
    }

    #[test]
    fn an_untrusted_peer_is_the_client_whatever_it_forwards() {
        let address = client_address(ip("203.0.113.5"), Some("10.9.9.9"), &proxies(""));

        assert_eq!(address, "203.0.113.5");
    }

    #[test]
    fn a_trusted_peer_names_the_forwarded_client() {
        let address = client_address(
            ip("172.18.0.1"),
            Some("100.64.0.7"),
            &proxies("172.16.0.0/12"),
        );

        assert_eq!(address, "100.64.0.7");
    }

    #[test]
    fn forged_entries_left_of_the_real_client_are_ignored() {
        let address = client_address(
            ip("172.18.0.1"),
            Some("1.2.3.4, 100.64.0.7, 172.18.0.5"),
            &proxies("172.16.0.0/12"),
        );

        assert_eq!(address, "100.64.0.7");
    }

    #[test]
    fn a_trusted_peer_without_the_header_is_the_client() {
        let address = client_address(ip("172.18.0.1"), None, &proxies("172.16.0.0/12"));

        assert_eq!(address, "172.18.0.1");
    }

    #[test]
    fn a_malformed_hop_stops_the_walk_at_the_peer() {
        let address = client_address(
            ip("172.18.0.1"),
            Some("100.64.0.7, garbage"),
            &proxies("172.16.0.0/12"),
        );

        assert_eq!(address, "172.18.0.1");
    }

    #[test]
    fn an_ipv4_mapped_peer_is_its_ipv4_address() {
        assert_eq!(
            client_address(ip("::ffff:203.0.113.5"), None, &[]),
            "203.0.113.5"
        );
    }

    #[test]
    fn an_ipv4_mapped_peer_matches_an_ipv4_proxy_range() {
        let address = client_address(
            ip("::ffff:172.18.0.1"),
            Some("::ffff:100.64.0.7"),
            &proxies("172.16.0.0/12"),
        );

        assert_eq!(address, "100.64.0.7");
    }

    #[test]
    fn a_missing_peer_is_unknown() {
        assert_eq!(client_address(None, None, &[]), "unknown");
    }
}
