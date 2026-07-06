//! Connect-Timeout-Ms parsing and computation.
//!
//! This module provides support for the Connect protocol's timeout mechanism.
//! Clients can set a `Connect-Timeout-Ms` header to specify how long they're
//! willing to wait for a response.

use crate::message::error::{Code, ConnectError};
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

    /// Create a ConnectTimeout from an optional duration.
    pub fn from_duration(duration: Option<Duration>) -> Self {
        Self { duration }
    }

    /// Returns the timeout duration, or `None` if no timeout was specified.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// Parse the Connect-Timeout-Ms header value.
    ///
    /// Returns `Some(ConnectTimeout)` with the parsed duration if the value is valid,
    /// or `None` if the value was invalid.
    ///
    /// Per the Connect spec, the value must be a non-negative integer with at
    /// most 10 digits representing milliseconds. A value of `0` is an
    /// immediately-expired deadline, not "no timeout".
    pub fn parse(value: &str) -> Option<Self> {
        parse_timeout_ms(value).ok().map(Self::new)
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
/// Returns `Ok(Some(Duration))` if the header is present and valid,
/// `Ok(None)` if the header is missing or empty, or an `InvalidArgument`
/// error if the value is malformed (matching connect-go).
pub fn parse_timeout<B>(req: &Request<B>) -> Result<Option<Duration>, ConnectError> {
    let Some(value) = req.headers().get(CONNECT_TIMEOUT_MS_HEADER) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| {
        ConnectError::new(Code::InvalidArgument, "parse timeout: invalid header value")
    })?;
    if value.is_empty() {
        return Ok(None);
    }
    parse_timeout_ms(value).map(Some)
}

/// Parse a timeout milliseconds string.
///
/// Returns `Ok(Duration)` for valid values, or an `InvalidArgument` error for
/// unparseable values or values with more than 10 digits (matching connect-go).
/// A value of `0` is a valid, immediately-expired deadline.
pub fn parse_timeout_ms(value: &str) -> Result<Duration, ConnectError> {
    if value.len() > 10 {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!("parse timeout: \"{value}\" has >10 digits"),
        ));
    }
    let ms: u64 = value
        .parse()
        .map_err(|e| ConnectError::new(Code::InvalidArgument, format!("parse timeout: {e}")))?;
    Ok(Duration::from_millis(ms))
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
        // 0 is an immediately-expired deadline, not "no timeout"
        let timeout = ConnectTimeout::parse("0").unwrap();
        assert_eq!(timeout.duration(), Some(Duration::ZERO));
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
            parse_timeout_ms("1000").unwrap(),
            Duration::from_millis(1000)
        );
        assert_eq!(
            parse_timeout_ms("5000").unwrap(),
            Duration::from_millis(5000)
        );
    }

    #[test]
    fn test_parse_timeout_ms_zero() {
        // 0 is a valid, immediately-expired deadline
        assert_eq!(parse_timeout_ms("0").unwrap(), Duration::ZERO);
    }

    #[test]
    fn test_parse_timeout_ms_invalid() {
        assert_eq!(
            parse_timeout_ms("abc").unwrap_err().code(),
            Code::InvalidArgument
        );
        assert_eq!(
            parse_timeout_ms("-1").unwrap_err().code(),
            Code::InvalidArgument
        );
        assert_eq!(
            parse_timeout_ms("").unwrap_err().code(),
            Code::InvalidArgument
        );
    }

    #[test]
    fn test_parse_timeout_ms_too_many_digits() {
        // connect-go rejects values with more than 10 digits
        let err = parse_timeout_ms("12345678901").unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().unwrap().contains(">10 digits"));
        // 10 digits is fine
        assert!(parse_timeout_ms("1234567890").is_ok());
    }

    // --- parse_timeout tests ---

    #[test]
    fn test_parse_timeout_valid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "5000")
            .body(())
            .unwrap();
        assert_eq!(
            parse_timeout(&req).unwrap(),
            Some(Duration::from_millis(5000))
        );
    }

    #[test]
    fn test_parse_timeout_zero() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "0")
            .body(())
            .unwrap();
        assert_eq!(parse_timeout(&req).unwrap(), Some(Duration::ZERO));
    }

    #[test]
    fn test_parse_timeout_missing() {
        let req = Request::builder().method(Method::POST).body(()).unwrap();
        assert_eq!(parse_timeout(&req).unwrap(), None);
    }

    #[test]
    fn test_parse_timeout_empty() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "")
            .body(())
            .unwrap();
        assert_eq!(parse_timeout(&req).unwrap(), None);
    }

    #[test]
    fn test_parse_timeout_invalid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_TIMEOUT_MS_HEADER, "not-a-number")
            .body(())
            .unwrap();
        let err = parse_timeout(&req).unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
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
