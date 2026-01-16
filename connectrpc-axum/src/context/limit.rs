//! Message size limits for Connect RPC requests and responses.
//!
//! This module provides configuration for limiting message sizes to prevent
//! memory exhaustion attacks. The default limit of 4 MB matches gRPC's default.
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

/// Default maximum message size (4 MB), matching gRPC's default receive limit.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Configuration for message size limits.
///
/// These limits are enforced on both incoming requests and outgoing responses
/// to prevent memory exhaustion and protect clients from oversized messages.
///
/// # Example
///
/// ```rust
/// use connectrpc_axum::MessageLimits;
///
/// // Use default 4 MB receive limit (no send limit)
/// let limits = MessageLimits::default();
///
/// // Custom limits for both directions
/// let limits = MessageLimits::default()
///     .receive_max_bytes(16 * 1024 * 1024)  // 16 MB for requests
///     .send_max_bytes(8 * 1024 * 1024);     // 8 MB for responses
///
/// // No limits (not recommended for production)
/// let limits = MessageLimits::unlimited();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageLimits {
    /// Maximum size of incoming messages in bytes.
    /// `None` means unlimited (not recommended).
    receive_max_bytes: Option<usize>,
    /// Maximum size of outgoing messages in bytes.
    /// `None` means unlimited (default).
    send_max_bytes: Option<usize>,
}

impl Default for MessageLimits {
    fn default() -> Self {
        Self {
            receive_max_bytes: Some(DEFAULT_MAX_MESSAGE_SIZE),
            send_max_bytes: None, // No send limit by default (matching connect-go)
        }
    }
}

impl MessageLimits {
    /// Create new limits with the specified maximum receive message size in bytes.
    ///
    /// This is a convenience constructor that sets only the receive limit.
    /// For more control, use the builder methods.
    pub fn new(max_message_size: usize) -> Self {
        Self {
            receive_max_bytes: Some(max_message_size),
            send_max_bytes: None,
        }
    }

    /// Create limits with no maximum (not recommended for production).
    ///
    /// # Security Warning
    ///
    /// Using unlimited message sizes can allow attackers to exhaust server
    /// memory with large requests. Only use this in trusted environments.
    pub fn unlimited() -> Self {
        Self {
            receive_max_bytes: None,
            send_max_bytes: None,
        }
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
    #[deprecated(since = "0.3.0", note = "Use `receive_max_bytes()` instead")]
    pub fn max_message_size(&self) -> Option<usize> {
        self.receive_max_bytes
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
    #[deprecated(since = "0.3.0", note = "Use `receive_max_bytes_or_max()` instead")]
    pub fn max_message_size_or_max(&self) -> usize {
        self.receive_max_bytes.unwrap_or(usize::MAX)
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
    fn test_default_limits() {
        let limits = MessageLimits::default();
        assert_eq!(
            limits.get_receive_max_bytes(),
            Some(DEFAULT_MAX_MESSAGE_SIZE)
        );
        assert_eq!(limits.get_send_max_bytes(), None); // No send limit by default
    }

    #[test]
    fn test_custom_limits() {
        let limits = MessageLimits::new(1024);
        assert_eq!(limits.get_receive_max_bytes(), Some(1024));
        assert_eq!(limits.get_send_max_bytes(), None);
    }

    #[test]
    fn test_builder_methods() {
        let limits = MessageLimits::default()
            .receive_max_bytes(2048)
            .send_max_bytes(1024);
        assert_eq!(limits.get_receive_max_bytes(), Some(2048));
        assert_eq!(limits.get_send_max_bytes(), Some(1024));
    }

    #[test]
    fn test_unlimited() {
        let limits = MessageLimits::unlimited();
        assert_eq!(limits.get_receive_max_bytes(), None);
        assert_eq!(limits.get_send_max_bytes(), None);
        assert_eq!(limits.receive_max_bytes_or_max(), usize::MAX);
    }

    #[test]
    fn test_check_size_within_limit() {
        let limits = MessageLimits::new(1024);
        assert!(limits.check_size(512).is_ok());
        assert!(limits.check_size(1024).is_ok());
    }

    #[test]
    fn test_check_size_exceeds_limit() {
        let limits = MessageLimits::new(1024);
        let result = limits.check_size(1025);
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        assert!(err_msg.contains("1025"));
        assert!(err_msg.contains("1024"));
    }

    #[test]
    fn test_check_size_connect_exceeds_limit() {
        let limits = MessageLimits::new(1024);
        let result = limits.check_size_connect(1025);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err.code(), Code::ResourceExhausted));
    }

    #[test]
    fn test_check_size_unlimited() {
        let limits = MessageLimits::unlimited();
        assert!(limits.check_size(usize::MAX).is_ok());
    }

    #[test]
    fn test_check_send_size_within_limit() {
        let limits = MessageLimits::default().send_max_bytes(1024);
        assert!(limits.check_send_size(512).is_ok());
        assert!(limits.check_send_size(1024).is_ok());
    }

    #[test]
    fn test_check_send_size_exceeds_limit() {
        let limits = MessageLimits::default().send_max_bytes(1024);
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
        let limits = MessageLimits::default(); // No send limit by default
        assert!(limits.check_send_size(usize::MAX).is_ok());
    }

    #[test]
    fn test_check_send_size_unlimited() {
        let limits = MessageLimits::unlimited();
        assert!(limits.check_send_size(usize::MAX).is_ok());
    }
}
