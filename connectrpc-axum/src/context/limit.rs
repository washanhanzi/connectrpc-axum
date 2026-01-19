//! Message size limits for Connect RPC requests and responses.
//!
//! This module provides configuration for limiting message sizes to prevent
//! memory exhaustion attacks.
//!
//! # Receive vs Send Limits
//!
//! - **Receive limit** (`receive_max_bytes`): Limits incoming request message sizes.
//!   Prevents clients from sending oversized requests that could exhaust server memory.
//!
//! - **Send limit** (`send_max_bytes`): Limits outgoing response message sizes.
//!   Prevents the server from accidentally sending oversized responses that could
//!   overwhelm clients or violate protocol constraints.

use crate::error::{Code, ConnectError};

/// Configuration for message size limits.
///
/// These limits are enforced on both incoming requests and outgoing responses
/// to prevent memory exhaustion and protect clients from oversized messages.
///
/// By default, no limits are applied. Use the builder methods to set limits.
///
/// # Example
///
/// ```rust
/// use connectrpc_axum::MessageLimits;
///
/// // Set receive limit only
/// let limits = MessageLimits::new().receive_max_bytes(4 * 1024 * 1024);
///
/// // Set both receive and send limits
/// let limits = MessageLimits::new()
///     .receive_max_bytes(16 * 1024 * 1024)  // 16 MB for requests
///     .send_max_bytes(8 * 1024 * 1024);     // 8 MB for responses
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageLimits {
    /// Maximum size of incoming messages in bytes.
    receive_max_bytes: Option<usize>,
    /// Maximum size of outgoing messages in bytes.
    send_max_bytes: Option<usize>,
}

impl MessageLimits {
    /// Create new limits with no restrictions.
    ///
    /// Use builder methods to set specific limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum size for incoming (receive) messages.
    ///
    /// This limits the size of request messages that clients can send.
    /// Returns `ResourceExhausted` error if a request exceeds this limit.
    pub fn receive_max_bytes(mut self, max: usize) -> Self {
        self.receive_max_bytes = Some(max);
        self
    }

    /// Set the maximum size for outgoing (send) messages.
    ///
    /// This limits the size of response messages that the server can send.
    /// Returns `ResourceExhausted` error if a response would exceed this limit.
    ///
    /// Following connect-go's behavior, the size is checked after encoding
    /// and after compression (if compression is applied).
    pub fn send_max_bytes(mut self, max: usize) -> Self {
        self.send_max_bytes = Some(max);
        self
    }

    /// Returns the maximum receive message size, or `None` if unlimited.
    pub fn get_receive_max_bytes(&self) -> Option<usize> {
        self.receive_max_bytes
    }

    /// Returns the maximum send message size, or `None` if unlimited.
    pub fn get_send_max_bytes(&self) -> Option<usize> {
        self.send_max_bytes
    }

    /// Returns the maximum receive message size for use with axum::body::to_bytes.
    ///
    /// Returns `usize::MAX` if unlimited.
    pub fn receive_max_bytes_or_max(&self) -> usize {
        self.receive_max_bytes.unwrap_or(usize::MAX)
    }

    /// Check if an incoming message size exceeds the configured receive limit.
    ///
    /// Returns `Ok(())` if the size is within limits, or `Err(String)` if it exceeds.
    /// Use this variant when you need to customize the error handling.
    pub fn check_size(&self, size: usize) -> Result<(), String> {
        if let Some(max) = self.receive_max_bytes
            && size > max
        {
            return Err(format!(
                "message size {} bytes exceeds maximum allowed size of {} bytes",
                size, max
            ));
        }
        Ok(())
    }

    /// Check if an incoming message size exceeds the configured receive limit.
    ///
    /// Returns `Ok(())` if the size is within limits, or `Err(ConnectError)` if it exceeds.
    pub fn check_size_connect(&self, size: usize) -> Result<(), ConnectError> {
        if let Some(max) = self.receive_max_bytes
            && size > max
        {
            return Err(ConnectError::new(
                Code::ResourceExhausted,
                format!(
                    "message size {} bytes exceeds maximum allowed size of {} bytes",
                    size, max
                ),
            ));
        }
        Ok(())
    }

    /// Check if an outgoing message size exceeds the configured send limit.
    ///
    /// Returns `Ok(())` if the size is within limits, or `Err(ConnectError)` if it exceeds.
    ///
    /// This follows connect-go's behavior of returning `CodeResourceExhausted` when
    /// a response message would exceed the send limit.
    pub fn check_send_size(&self, size: usize) -> Result<(), ConnectError> {
        if let Some(max) = self.send_max_bytes
            && size > max
        {
            return Err(ConnectError::new(
                Code::ResourceExhausted,
                format!("message size {} exceeds sendMaxBytes {}", size, max),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_no_limits() {
        let limits = MessageLimits::default();
        assert_eq!(limits.get_receive_max_bytes(), None);
        assert_eq!(limits.get_send_max_bytes(), None);
    }

    #[test]
    fn test_builder_methods() {
        let limits = MessageLimits::new()
            .receive_max_bytes(2048)
            .send_max_bytes(1024);
        assert_eq!(limits.get_receive_max_bytes(), Some(2048));
        assert_eq!(limits.get_send_max_bytes(), Some(1024));
    }

    #[test]
    fn test_receive_max_bytes_or_max() {
        let limits = MessageLimits::new();
        assert_eq!(limits.receive_max_bytes_or_max(), usize::MAX);

        let limits = MessageLimits::new().receive_max_bytes(1024);
        assert_eq!(limits.receive_max_bytes_or_max(), 1024);
    }

    #[test]
    fn test_check_size_within_limit() {
        let limits = MessageLimits::new().receive_max_bytes(1024);
        assert!(limits.check_size(512).is_ok());
        assert!(limits.check_size(1024).is_ok());
    }

    #[test]
    fn test_check_size_exceeds_limit() {
        let limits = MessageLimits::new().receive_max_bytes(1024);
        let result = limits.check_size(1025);
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        assert!(err_msg.contains("1025"));
        assert!(err_msg.contains("1024"));
    }

    #[test]
    fn test_check_size_connect_exceeds_limit() {
        let limits = MessageLimits::new().receive_max_bytes(1024);
        let result = limits.check_size_connect(1025);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err.code(), Code::ResourceExhausted));
    }

    #[test]
    fn test_check_size_no_limit() {
        let limits = MessageLimits::new();
        assert!(limits.check_size(usize::MAX).is_ok());
    }

    #[test]
    fn test_check_send_size_within_limit() {
        let limits = MessageLimits::new().send_max_bytes(1024);
        assert!(limits.check_send_size(512).is_ok());
        assert!(limits.check_send_size(1024).is_ok());
    }

    #[test]
    fn test_check_send_size_exceeds_limit() {
        let limits = MessageLimits::new().send_max_bytes(1024);
        let result = limits.check_send_size(1025);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err.code(), Code::ResourceExhausted));
        let message = err.message().unwrap();
        assert!(message.contains("1025"));
        assert!(message.contains("1024"));
    }

    #[test]
    fn test_check_send_size_no_limit() {
        let limits = MessageLimits::new();
        assert!(limits.check_send_size(usize::MAX).is_ok());
    }
}
