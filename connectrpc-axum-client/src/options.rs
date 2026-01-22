//! Call options for per-request configuration.
//!
//! This module provides [`CallOptions`] for configuring individual RPC calls
//! with timeouts, custom headers, and other per-call settings.

use http::{HeaderMap, HeaderName, HeaderValue};
use std::time::Duration;

/// Options for configuring individual RPC calls.
///
/// Use this to set per-call timeouts, custom headers, or other request-specific
/// configuration that differs from the client defaults.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::CallOptions;
/// use std::time::Duration;
///
/// let options = CallOptions::new()
///     .timeout(Duration::from_secs(5))
///     .header("authorization", "Bearer token123")
///     .header("x-request-id", "abc-123");
///
/// let response = client.call_unary_with_options::<Req, Res>(
///     "my.service/Method",
///     &request,
///     options,
/// ).await?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    /// Timeout for this specific call.
    /// If set, overrides the client's default timeout.
    pub(crate) timeout: Option<Duration>,
    /// Custom headers for this specific call.
    pub(crate) headers: HeaderMap,
}

impl CallOptions {
    /// Create new default call options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the timeout for this call.
    ///
    /// This timeout is propagated to the server via the `Connect-Timeout-Ms` header,
    /// allowing the server to cancel processing if the deadline will be exceeded.
    ///
    /// The maximum supported timeout is approximately 115 days (10 digit milliseconds).
    /// Larger values will be treated as no timeout.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    /// use std::time::Duration;
    ///
    /// let options = CallOptions::new()
    ///     .timeout(Duration::from_secs(30));
    /// ```
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Get the configured timeout, if any.
    pub fn get_timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Add a custom header for this call.
    ///
    /// Headers beginning with "Connect-" and "Grpc-" are reserved for use by
    /// the Connect and gRPC protocols. Applications may read them but should
    /// not write them.
    ///
    /// # Panics
    ///
    /// Panics if the header name or value is invalid.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    ///
    /// let options = CallOptions::new()
    ///     .header("authorization", "Bearer token123")
    ///     .header("x-request-id", "abc-123");
    /// ```
    pub fn header<K, V>(mut self, name: K, value: V) -> Self
    where
        K: TryInto<HeaderName>,
        K::Error: std::fmt::Debug,
        V: TryInto<HeaderValue>,
        V::Error: std::fmt::Debug,
    {
        let name = name.try_into().expect("invalid header name");
        let value = value.try_into().expect("invalid header value");
        self.headers.insert(name, value);
        self
    }

    /// Try to add a custom header for this call.
    ///
    /// Returns `None` if the header name or value is invalid.
    ///
    /// Headers beginning with "Connect-" and "Grpc-" are reserved for use by
    /// the Connect and gRPC protocols. Applications may read them but should
    /// not write them.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    ///
    /// let options = CallOptions::new()
    ///     .try_header("authorization", "Bearer token123")?
    ///     .try_header("x-request-id", "abc-123")?;
    /// ```
    pub fn try_header<K, V>(mut self, name: K, value: V) -> Option<Self>
    where
        K: TryInto<HeaderName>,
        V: TryInto<HeaderValue>,
    {
        let name = name.try_into().ok()?;
        let value = value.try_into().ok()?;
        self.headers.insert(name, value);
        Some(self)
    }

    /// Set all custom headers for this call, replacing any existing headers.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    /// use http::HeaderMap;
    ///
    /// let mut headers = HeaderMap::new();
    /// headers.insert("authorization", "Bearer token123".parse().unwrap());
    ///
    /// let options = CallOptions::new().headers(headers);
    /// ```
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    /// Get a reference to the custom headers.
    pub fn get_headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the custom headers.
    ///
    /// This allows direct manipulation of the header map.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    ///
    /// let mut options = CallOptions::new();
    /// options.headers_mut().insert("x-custom", "value".parse().unwrap());
    /// ```
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }
}

/// Maximum timeout value in milliseconds (10 digits = 9,999,999,999 ms ≈ 115 days).
/// Values larger than this are treated as "no timeout" per Connect protocol spec.
pub(crate) const MAX_TIMEOUT_MS: u128 = 9_999_999_999;

/// Convert a Duration to the Connect-Timeout-Ms header value.
///
/// Returns None if the timeout is too large (> 10 digits) or zero/negative.
pub(crate) fn duration_to_timeout_header(duration: Duration) -> Option<String> {
    let millis = duration.as_millis();
    if millis == 0 || millis > MAX_TIMEOUT_MS {
        return None;
    }
    Some(millis.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_options_default() {
        let options = CallOptions::new();
        assert!(options.timeout.is_none());
    }

    #[test]
    fn test_call_options_timeout() {
        let options = CallOptions::new().timeout(Duration::from_secs(30));
        assert_eq!(options.timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_call_options_header() {
        let options = CallOptions::new()
            .header("authorization", "Bearer token123")
            .header("x-request-id", "abc-123");

        assert_eq!(
            options.headers.get("authorization").unwrap(),
            "Bearer token123"
        );
        assert_eq!(options.headers.get("x-request-id").unwrap(), "abc-123");
    }

    #[test]
    fn test_call_options_try_header() {
        let options = CallOptions::new()
            .try_header("authorization", "Bearer token")
            .unwrap()
            .try_header("x-custom", "value")
            .unwrap();

        assert_eq!(options.headers.get("authorization").unwrap(), "Bearer token");
        assert_eq!(options.headers.get("x-custom").unwrap(), "value");
    }

    #[test]
    fn test_call_options_try_header_invalid() {
        // Invalid header name (contains invalid characters)
        let result = CallOptions::new().try_header("invalid\0name", "value");
        assert!(result.is_none());
    }

    #[test]
    fn test_call_options_headers_map() {
        use http::HeaderMap;

        let mut headers = HeaderMap::new();
        headers.insert("x-custom", "value".parse().unwrap());

        let options = CallOptions::new().headers(headers);
        assert_eq!(options.headers.get("x-custom").unwrap(), "value");
    }

    #[test]
    fn test_call_options_headers_mut() {
        let mut options = CallOptions::new();
        options
            .headers_mut()
            .insert("x-custom", "value".parse().unwrap());
        assert_eq!(options.headers.get("x-custom").unwrap(), "value");
    }

    #[test]
    fn test_call_options_combined() {
        let options = CallOptions::new()
            .timeout(Duration::from_secs(30))
            .header("authorization", "Bearer token");

        assert_eq!(options.timeout, Some(Duration::from_secs(30)));
        assert_eq!(
            options.headers.get("authorization").unwrap(),
            "Bearer token"
        );
    }

    #[test]
    fn test_duration_to_timeout_header() {
        // Normal case
        assert_eq!(
            duration_to_timeout_header(Duration::from_secs(30)),
            Some("30000".to_string())
        );

        // 1 millisecond
        assert_eq!(
            duration_to_timeout_header(Duration::from_millis(1)),
            Some("1".to_string())
        );

        // Max valid (10 digits)
        assert_eq!(
            duration_to_timeout_header(Duration::from_millis(9_999_999_999)),
            Some("9999999999".to_string())
        );

        // Too large (11 digits) - returns None
        assert_eq!(
            duration_to_timeout_header(Duration::from_millis(10_000_000_000)),
            None
        );

        // Zero - returns None
        assert_eq!(duration_to_timeout_header(Duration::ZERO), None);
    }
}
