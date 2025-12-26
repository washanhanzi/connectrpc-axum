//! Message size limits for Connect RPC requests.
//!
//! This module provides configuration for limiting message sizes to prevent
//! memory exhaustion attacks. The default limit of 4 MB matches gRPC's default.

/// Default maximum message size (4 MB), matching gRPC's default receive limit.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Configuration for message size limits.
///
/// These limits are enforced when parsing incoming requests to prevent
/// memory exhaustion from maliciously large messages.
///
/// # Example
///
/// ```rust
/// use connectrpc_axum::MessageLimits;
///
/// // Use default 4 MB limit
/// let limits = MessageLimits::default();
///
/// // Custom 16 MB limit for large payloads
/// let limits = MessageLimits::new(16 * 1024 * 1024);
///
/// // No limit (not recommended for production)
/// let limits = MessageLimits::unlimited();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageLimits {
    /// Maximum size of a single message in bytes.
    /// `None` means unlimited (not recommended).
    max_message_size: Option<usize>,
}

impl Default for MessageLimits {
    fn default() -> Self {
        Self {
            max_message_size: Some(DEFAULT_MAX_MESSAGE_SIZE),
        }
    }
}

impl MessageLimits {
    /// Create new limits with the specified maximum message size in bytes.
    pub fn new(max_message_size: usize) -> Self {
        Self {
            max_message_size: Some(max_message_size),
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
            max_message_size: None,
        }
    }

    /// Returns the maximum message size, or `None` if unlimited.
    pub fn max_message_size(&self) -> Option<usize> {
        self.max_message_size
    }

    /// Check if a message size exceeds the configured limit.
    ///
    /// Returns `Ok(())` if the size is within limits, or `Err` with a
    /// descriptive error message if it exceeds the limit.
    pub fn check_size(&self, size: usize) -> Result<(), String> {
        if let Some(max) = self.max_message_size {
            if size > max {
                return Err(format!(
                    "message size {} bytes exceeds maximum allowed size of {} bytes",
                    size, max
                ));
            }
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
        assert_eq!(limits.max_message_size(), Some(DEFAULT_MAX_MESSAGE_SIZE));
    }

    #[test]
    fn test_custom_limits() {
        let limits = MessageLimits::new(1024);
        assert_eq!(limits.max_message_size(), Some(1024));
    }

    #[test]
    fn test_unlimited() {
        let limits = MessageLimits::unlimited();
        assert_eq!(limits.max_message_size(), None);
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
    fn test_check_size_unlimited() {
        let limits = MessageLimits::unlimited();
        assert!(limits.check_size(usize::MAX).is_ok());
    }
}
