//! Timeout parsing and computation for the Connect layer.

use crate::context::ConnectTimeout;
use axum::http::Request;
use std::time::Duration;

/// Header name for Connect timeout in milliseconds.
pub const CONNECT_TIMEOUT_MS_HEADER: &str = "connect-timeout-ms";

/// Parse the Connect-Timeout-Ms header from a request.
///
/// Returns a `ConnectTimeout` with the parsed duration, or a default (no timeout)
/// if the header is missing or invalid.
pub fn parse_timeout<B>(req: &Request<B>) -> ConnectTimeout {
    req.headers()
        .get(CONNECT_TIMEOUT_MS_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(ConnectTimeout::parse)
        .unwrap_or_default()
}

/// Compute the effective timeout from server and client timeouts.
///
/// The effective timeout is the minimum of the two, matching Connect-Go's behavior
/// where the smaller timeout always wins.
pub fn compute_effective_timeout(
    server_timeout: Option<Duration>,
    client_timeout: ConnectTimeout,
) -> ConnectTimeout {
    match (server_timeout, client_timeout.duration()) {
        // Both set: use the smaller
        (Some(server), Some(client)) => ConnectTimeout::new(server.min(client)),
        // Only server set
        (Some(server), None) => ConnectTimeout::new(server),
        // Only client set (or neither)
        (None, _) => client_timeout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request};

    #[test]
    fn test_parse_timeout_valid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "5000")
            .body(())
            .unwrap();
        let timeout = parse_timeout(&req);
        assert_eq!(timeout.duration(), Some(Duration::from_millis(5000)));
    }

    #[test]
    fn test_parse_timeout_zero() {
        // Zero means no timeout
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "0")
            .body(())
            .unwrap();
        let timeout = parse_timeout(&req);
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_parse_timeout_missing() {
        let req = Request::builder().method(Method::POST).body(()).unwrap();
        let timeout = parse_timeout(&req);
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_parse_timeout_invalid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "not-a-number")
            .body(())
            .unwrap();
        let timeout = parse_timeout(&req);
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_compute_effective_timeout_both_set_server_smaller() {
        let server = Some(Duration::from_secs(5));
        let client = ConnectTimeout::new(Duration::from_secs(10));
        let effective = compute_effective_timeout(server, client);
        // Server timeout is smaller, so it wins
        assert_eq!(effective.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_compute_effective_timeout_both_set_client_smaller() {
        let server = Some(Duration::from_secs(10));
        let client = ConnectTimeout::new(Duration::from_secs(5));
        let effective = compute_effective_timeout(server, client);
        // Client timeout is smaller, so it wins
        assert_eq!(effective.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_compute_effective_timeout_only_server() {
        let server = Some(Duration::from_secs(5));
        let client = ConnectTimeout::none();
        let effective = compute_effective_timeout(server, client);
        assert_eq!(effective.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_compute_effective_timeout_only_client() {
        let server = None;
        let client = ConnectTimeout::new(Duration::from_secs(5));
        let effective = compute_effective_timeout(server, client);
        assert_eq!(effective.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_compute_effective_timeout_neither() {
        let server = None;
        let client = ConnectTimeout::none();
        let effective = compute_effective_timeout(server, client);
        assert_eq!(effective.duration(), None);
    }
}
