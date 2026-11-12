//! Client IP address of a request, used as the key of the per-client limits.
//!
//! Forwarding headers are trusted only as far as the configured number of
//! reverse proxies in front of the server: every entry further left in
//! `X-Forwarded-For` was written by the client and can be anything.

use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request};
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Resolves the client IP from the request `headers`, the `peer` socket
/// address and the number of `trusted_hops` (reverse proxies in front of the
/// server, each appending the address it received the request from).
///
/// - `trusted_hops == 0`: every forwarding header is ignored and the result
///   is `peer`.
/// - `trusted_hops == n > 0`: only `X-Forwarded-For` is read (`X-Real-IP`
///   and `Forwarded` are ignored). Its comma-separated entries, across all
///   its header lines in order, are counted from the right and the n-th one
///   is the result: with one proxy, the last entry. No entry further left is
///   ever read.
///
/// Falls back to `peer` when the header is missing, has fewer than n
/// entries, has a line that is not visible ASCII, or the n-th entry is not a
/// bare IP address (an empty entry or one with a port is not). Returns
/// `None` only when that fallback is needed and `peer` is `None`.
pub fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    trusted_hops: u8,
) -> Option<IpAddr> {
    if trusted_hops == 0 {
        return peer;
    }
    let mut entries = Vec::new();
    for value in headers.get_all(X_FORWARDED_FOR) {
        let Ok(text) = value.to_str() else {
            return peer;
        };
        entries.extend(text.split(',').map(str::trim));
    }
    let index_from_right = usize::from(trusted_hops) - 1;
    let Some(entry) = entries.iter().rev().nth(index_from_right) else {
        return peer;
    };
    match entry.parse::<IpAddr>() {
        Ok(ip) => Some(ip),
        Err(_) => peer,
    }
}

/// Peer IP address of `request`, from the [`ConnectInfo`] extension that
/// `into_make_service_with_connect_info::<SocketAddr>` adds. `None` when the
/// server was not started with connect info.
pub fn peer_ip<T>(request: &Request<T>) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}

/// Rate limiter key: the client IP resolved by [`resolve_client_ip`] with
/// the peer address of [`peer_ip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIpKeyExtractor {
    /// Reverse proxies trusted to append to `X-Forwarded-For`.
    pub trusted_hops: u8,
}

impl KeyExtractor for ClientIpKeyExtractor {
    type Key = IpAddr;

    /// # Errors
    ///
    /// Returns [`GovernorError::UnableToExtractKey`] (answered with a 500)
    /// when no client IP can be resolved: no usable `X-Forwarded-For` entry
    /// and no peer address.
    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        resolve_client_ip(req.headers(), peer_ip(req), self.trusted_hops)
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    const PEER: &str = "192.0.2.10";

    fn peer() -> Option<IpAddr> {
        Some(PEER.parse().unwrap())
    }

    fn ip(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    fn headers_with_xff(lines: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append(X_FORWARDED_FOR, HeaderValue::from_str(line).unwrap());
        }
        headers
    }

    #[test]
    fn zero_hops_ignores_x_forwarded_for() {
        let headers = headers_with_xff(&["203.0.113.1"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 0), peer());
    }

    #[test]
    fn zero_hops_ignores_x_real_ip_and_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.1"));
        headers.insert("forwarded", HeaderValue::from_static("for=203.0.113.2"));

        assert_eq!(resolve_client_ip(&headers, peer(), 0), peer());
    }

    #[test]
    fn zero_hops_without_peer_is_none() {
        let headers = headers_with_xff(&["203.0.113.1"]);

        assert_eq!(resolve_client_ip(&headers, None, 0), None);
    }

    #[test]
    fn one_hop_takes_the_last_entry() {
        let headers = headers_with_xff(&["203.0.113.1, 198.51.100.7"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), ip("198.51.100.7"));
    }

    // The client writes the leftmost entry; the trusted proxy appends the
    // real address on the right.
    #[test]
    fn one_hop_ignores_a_spoofed_leftmost_entry() {
        let headers = headers_with_xff(&["6.6.6.6, 1.2.3.4, 198.51.100.7"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), ip("198.51.100.7"));
    }

    #[test]
    fn two_hops_take_the_second_entry_from_the_right() {
        let headers = headers_with_xff(&["6.6.6.6, 198.51.100.7, 10.0.0.2"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 2), ip("198.51.100.7"));
    }

    #[test]
    fn entries_are_collected_across_header_lines_in_order() {
        let headers = headers_with_xff(&["6.6.6.6, 198.51.100.7", "10.0.0.2"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 2), ip("198.51.100.7"));
    }

    #[test]
    fn last_header_line_holds_the_entry_of_one_hop() {
        let headers = headers_with_xff(&["6.6.6.6", "198.51.100.7"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), ip("198.51.100.7"));
    }

    #[test]
    fn missing_header_falls_back_to_peer() {
        assert_eq!(resolve_client_ip(&HeaderMap::new(), peer(), 1), peer());
    }

    #[test]
    fn fewer_entries_than_hops_fall_back_to_peer() {
        let headers = headers_with_xff(&["198.51.100.7"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 2), peer());
    }

    // A malformed entry is not skipped: the entry to its left was written by
    // an untrusted party.
    #[test]
    fn invalid_entry_falls_back_to_peer() {
        let headers = headers_with_xff(&["198.51.100.7, garbage"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), peer());
    }

    #[test]
    fn empty_entry_falls_back_to_peer() {
        let headers = headers_with_xff(&["198.51.100.7,"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), peer());
    }

    #[test]
    fn entry_with_port_falls_back_to_peer() {
        let headers = headers_with_xff(&["198.51.100.7:4000"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), peer());
    }

    #[test]
    fn non_ascii_header_line_falls_back_to_peer() {
        let mut headers = HeaderMap::new();
        headers.append(
            X_FORWARDED_FOR,
            HeaderValue::from_bytes(b"198.51.100.7, \xff").unwrap(),
        );

        assert_eq!(resolve_client_ip(&headers, peer(), 1), peer());
    }

    #[test]
    fn one_hop_ignores_x_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.1"));

        assert_eq!(resolve_client_ip(&headers, peer(), 1), peer());
    }

    #[test]
    fn one_hop_ignores_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", HeaderValue::from_static("for=203.0.113.2"));

        assert_eq!(resolve_client_ip(&headers, peer(), 1), peer());
    }

    #[test]
    fn unusable_header_without_peer_is_none() {
        let headers = headers_with_xff(&["garbage"]);

        assert_eq!(resolve_client_ip(&headers, None, 1), None);
    }

    #[test]
    fn ipv6_entry_is_accepted() {
        let headers = headers_with_xff(&["2001:db8::1"]);

        assert_eq!(resolve_client_ip(&headers, peer(), 1), ip("2001:db8::1"));
    }

    #[test]
    fn extractor_uses_connect_info_as_peer() {
        let extractor = ClientIpKeyExtractor { trusted_hops: 0 };
        let mut request = Request::new(());
        let addr: SocketAddr = format!("{PEER}:5555").parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(addr));

        assert_eq!(extractor.extract(&request).ok(), peer());
    }

    #[test]
    fn extractor_fails_without_any_address() {
        let extractor = ClientIpKeyExtractor { trusted_hops: 0 };
        let request = Request::new(());

        assert!(matches!(
            extractor.extract(&request),
            Err(GovernorError::UnableToExtractKey)
        ));
    }
}
