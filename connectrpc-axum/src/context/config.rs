//! Server configuration - server-wide static settings.
//!
//! Set once at startup, used to build Context for each request.

use crate::context::{CompressionConfig, MessageLimits};
use std::time::Duration;

/// Server-wide configuration for the Connect RPC layer.
///
/// Set once at startup, immutable per-request.
/// Used by ConnectLayer to build Context.
#[derive(Debug, Default, Clone, Copy)]
pub struct ServerConfig {
    /// Compression settings
    pub compression: CompressionConfig,
    /// Message size limits
    pub limits: MessageLimits,
    /// Server-side timeout (optional)
    pub server_timeout: Option<Duration>,
    /// Whether to require Connect-Protocol-Version header
    pub require_protocol_header: bool,
}

impl ServerConfig {
    /// Create a new server config with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the compression configuration.
    pub fn with_compression(mut self, config: CompressionConfig) -> Self {
        self.compression = config;
        self
    }

    /// Set the message size limits.
    pub fn with_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Set the server-side timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.server_timeout = Some(timeout);
        self
    }

    /// Require the Connect-Protocol-Version header.
    pub fn require_protocol_header(mut self) -> Self {
        self.require_protocol_header = true;
        self
    }
}
