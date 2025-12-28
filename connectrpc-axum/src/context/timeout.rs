//! Connect-Timeout-Ms parsing and enforcement.
//!
//! This module provides support for the Connect protocol's timeout mechanism.
//! Clients can set a `Connect-Timeout-Ms` header to specify how long they're
//! willing to wait for a response.

use std::time::Duration;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_timeout() {
        let timeout = ConnectTimeout::parse("1000").unwrap();
        assert_eq!(timeout.duration(), Some(Duration::from_millis(1000)));
    }

    #[test]
    fn test_parse_zero_timeout() {
        // 0 means no timeout
        let timeout = ConnectTimeout::parse("0").unwrap();
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_parse_invalid_timeout() {
        assert!(ConnectTimeout::parse("abc").is_none());
        assert!(ConnectTimeout::parse("-1").is_none());
        assert!(ConnectTimeout::parse("").is_none());
    }

    #[test]
    fn test_new_timeout() {
        let timeout = ConnectTimeout::new(Duration::from_secs(5));
        assert_eq!(timeout.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_none_timeout() {
        let timeout = ConnectTimeout::none();
        assert_eq!(timeout.duration(), None);
    }

    #[test]
    fn test_default_timeout() {
        let timeout = ConnectTimeout::default();
        assert_eq!(timeout.duration(), None);
    }
}
