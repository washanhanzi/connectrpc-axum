//! Connect-Timeout-Ms parsing and computation.
//!
//! This module provides support for the Connect protocol's timeout mechanism.
//! Clients can set a `Connect-Timeout-Ms` header to specify how long they're
//! willing to wait for a response.

use axum::http::Request;
use std::time::Duration;

/// Header name for Connect timeout in milliseconds.
pub const CONNECT_TIMEOUT_MS_HEADER: &str = "connect-timeout-ms";

// ============================================================================
// ConnectTimeout (backwards compatibility)
// ============================================================================

/// Timeout configuration extracted from the Connect-Timeout-Ms header.
///
/// This is stored in request extensions by [`ConnectLayer`](crate::ConnectLayer)
/// and can be used by handlers to enforce request timeouts.
///
/// # Example
///
/// ```rust,ignore
/// use connectrpc_axum::ConnectTimeout;
///
/// async fn handler(
///     timeout: Option<Extension<ConnectTimeout>>,
///     req: ConnectRequest<MyRequest>,
/// ) -> Result<ConnectResponse<MyResponse>, ConnectError> {
///     if let Some(Extension(timeout)) = timeout {
///         if let Some(duration) = timeout.duration() {
///             // Apply timeout to your operation
///         }
///     }
///     // ...
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectTimeout {
    /// The timeout duration, if specified and valid.
    duration: Option<Duration>,
}

impl ConnectTimeout {
    /// Create a new ConnectTimeout with the specified duration.
    pub fn new(duration: Duration) -> Self {
        Self {
            duration: Some(duration),
        }
    }

    /// Create a ConnectTimeout representing no timeout (unlimited).
    pub fn none() -> Self {
        Self { duration: None }
    }

    /// Returns the timeout duration, or `None` if no timeout was specified.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// Parse the Connect-Timeout-Ms header value.
    ///
    /// Returns `Some(ConnectTimeout)` with the parsed duration if the value is valid,
    /// or `None` if the header was not present or the value was invalid.
    ///
    /// Per the Connect spec, the value must be a non-negative integer representing
    /// milliseconds. Values of 0 mean "no timeout" (unlimited time).
    pub fn parse(value: &str) -> Option<Self> {
        let ms: u64 = value.parse().ok()?;
        if ms == 0 {
            // 0 means no timeout per Connect spec
            Some(Self::none())
        } else {
            Some(Self::new(Duration::from_millis(ms)))
        }
    }
}

impl Default for ConnectTimeout {
    fn default() -> Self {
        Self::none()
    }
}

// ============================================================================
// Parsing functions
// ============================================================================

/// Parse the Connect-Timeout-Ms header from a request.
///
/// Returns `Some(Duration)` if the header is present and valid,
/// or `None` if the header is missing, invalid, or zero (which means no timeout per Connect spec).
pub fn parse_timeout<B>(req: &Request<B>) -> Option<Duration> {
    req.headers()
        .get(CONNECT_TIMEOUT_MS_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_timeout_ms)
}

/// Parse a timeout milliseconds string.
///
/// Returns `Some(Duration)` for valid positive values,
/// or `None` for invalid values or 0 (which means no timeout per Connect spec).
pub fn parse_timeout_ms(value: &str) -> Option<Duration> {
    let ms: u64 = value.parse().ok()?;
    if ms == 0 {
        // 0 means no timeout per Connect spec
        None
    } else {
        Some(Duration::from_millis(ms))
    }
}

/// Compute the effective timeout from server and client timeouts.
///
/// The effective timeout is the minimum of the two, matching Connect-Go's behavior
/// where the smaller timeout always wins.
///
/// Returns `None` if neither timeout is set (unlimited).
pub fn compute_effective_timeout(
    server_timeout: Option<Duration>,
    client_timeout: Option<Duration>,
) -> Option<Duration> {
    match (server_timeout, client_timeout) {
        // Both set: use the smaller
        (Some(server), Some(client)) => Some(server.min(client)),
        // Only one set
        (Some(server), None) => Some(server),
        (None, Some(client)) => Some(client),
        // Neither set
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    // --- ConnectTimeout tests ---

    #[test]
    fn test_connect_timeout_new() {
        let timeout = ConnectTimeout::new(Duration::from_secs(5));
        assert_eq!(timeout.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_connect_timeout_none() {
        let timeout = ConnectTimeout::none();
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_connect_timeout_default() {
        let timeout = ConnectTimeout::default();
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_connect_timeout_parse_valid() {
        let timeout = ConnectTimeout::parse("1000").unwrap();
        assert_eq!(timeout.duration(), Some(Duration::from_millis(1000)));
    }

    #[test]
    fn test_connect_timeout_parse_zero() {
        // 0 means no timeout
        let timeout = ConnectTimeout::parse("0").unwrap();
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_connect_timeout_parse_invalid() {
        assert!(ConnectTimeout::parse("abc").is_none());
        assert!(ConnectTimeout::parse("-1").is_none());
        assert!(ConnectTimeout::parse("").is_none());
    }

    // --- parse_timeout_ms tests ---

    #[test]
    fn test_parse_timeout_ms_valid() {
        assert_eq!(
            parse_timeout_ms("1000"),
            Some(Duration::from_millis(1000))
        );
        assert_eq!(
            parse_timeout_ms("5000"),
            Some(Duration::from_millis(5000))
        );
    }

    #[test]
    fn test_parse_timeout_ms_zero() {
        // 0 means no timeout per Connect spec
        assert_eq!(parse_timeout_ms("0"), None);
    }

    #[test]
    fn test_parse_timeout_ms_invalid() {
        assert_eq!(parse_timeout_ms("abc"), None);
        assert_eq!(parse_timeout_ms("-1"), None);
        assert_eq!(parse_timeout_ms(""), None);
    }

    // --- parse_timeout tests ---

    #[test]
    fn test_parse_timeout_valid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "5000")
            .body(())
            .unwrap();
        assert_eq!(parse_timeout(&req), Some(Duration::from_millis(5000)));
    }

    #[test]
    fn test_parse_timeout_zero() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "0")
            .body(())
            .unwrap();
        assert_eq!(parse_timeout(&req), None);
    }

    #[test]
    fn test_parse_timeout_missing() {
        let req = Request::builder().method(Method::POST).body(()).unwrap();
        assert_eq!(parse_timeout(&req), None);
    }

    #[test]
    fn test_parse_timeout_invalid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "not-a-number")
            .body(())
            .unwrap();
        assert_eq!(parse_timeout(&req), None);
    }

    // --- compute_effective_timeout tests ---

    #[test]
    fn test_compute_effective_timeout_both_set_server_smaller() {
        let server = Some(Duration::from_secs(5));
        let client = Some(Duration::from_secs(10));
        assert_eq!(
            compute_effective_timeout(server, client),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn test_compute_effective_timeout_both_set_client_smaller() {
        let server = Some(Duration::from_secs(10));
        let client = Some(Duration::from_secs(5));
        assert_eq!(
            compute_effective_timeout(server, client),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn test_compute_effective_timeout_only_server() {
        let server = Some(Duration::from_secs(5));
        let client = None;
        assert_eq!(
            compute_effective_timeout(server, client),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn test_compute_effective_timeout_only_client() {
        let server = None;
        let client = Some(Duration::from_secs(5));
        assert_eq!(
            compute_effective_timeout(server, client),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn test_compute_effective_timeout_neither() {
        assert_eq!(compute_effective_timeout(None, None), None);
    }
}
