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
pub(crate) struct ServerConfig {
    /// Compression settings
    pub compression: CompressionConfig,
    /// Message size limits
    pub limits: MessageLimits,
    /// Server-side timeout (optional)
    pub server_timeout: Option<Duration>,
    /// Whether to require Connect-Protocol-Version header
    pub require_protocol_header: bool,
}
