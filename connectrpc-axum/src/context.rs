//! Context and common types for Connect RPC request handling.
//!
//! This module provides types used by the [`ConnectLayer`] middleware
//! and request extensions, including protocol detection, timeout configuration,
//! compression, and message size limits.
//!
//! [`ConnectLayer`]: crate::layer::ConnectLayer

pub mod config;
pub mod envelope_compression;
pub mod error;
pub mod limit;
pub mod protocol;
pub mod timeout;

use axum::http::{Method, Request};
use std::time::Duration;

// Re-export compression types and functions from envelope_compression
pub use envelope_compression::{
    // Boxed codec
    BoxedCodec,
    // Header constants
    CONNECT_ACCEPT_ENCODING,
    CONNECT_CONTENT_ENCODING,
    // Trait
    Codec,
    // Context types
    CompressionConfig,
    CompressionEncoding,
    CompressionLevel,
    EnvelopeCompression,
    // Built-in codecs
    GzipCodec,
    // Functions
    compress_bytes,
    decompress_bytes,
    negotiate_response_encoding,
    parse_envelope_compression,
    resolve_codec,
};

// Feature-gated codec exports
#[cfg(feature = "compression-br")]
pub use envelope_compression::BrotliCodec;
#[cfg(feature = "compression-deflate")]
pub use envelope_compression::DeflateCodec;
#[cfg(feature = "compression-zstd")]
pub use envelope_compression::ZstdCodec;

// Re-export config types (crate-internal)
pub(crate) use config::ServerConfig;

// Re-export error types
pub use error::{ContextError, ProtocolNegotiationError};

// Re-export limit types
pub use limit::MessageLimits;

// Re-export protocol types and functions
pub use protocol::{
    CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_HEADER, IdempotencyLevel, RequestProtocol,
    SUPPORTED_CONTENT_TYPES, can_handle_content_type, can_handle_get_encoding, detect_protocol,
    validate_content_type, validate_get_query_params, validate_protocol_version,
    validate_streaming_content_type, validate_unary_content_type,
};

// Re-export timeout types and functions
pub use timeout::{
    CONNECT_TIMEOUT_MS_HEADER, ConnectTimeout, compute_effective_timeout, parse_timeout,
    parse_timeout_ms,
};

// ============================================================================
// Context - per-request negotiated state
// ============================================================================

/// Per-request context built from headers and server config.
///
/// Created by ConnectLayer, stored in request extensions.
/// Used by pipelines to process messages.
#[derive(Debug, Default, Clone)]
pub struct ConnectContext {
    /// Protocol variant (unary/streaming, json/proto)
    pub protocol: RequestProtocol,
    /// Compression settings
    pub compression: CompressionContext,
    /// Effective timeout (min of server and client)
    pub timeout: Option<Duration>,
    /// Message size limits
    pub limits: MessageLimits,
    /// Whether protocol version header is required
    pub require_protocol_header: bool,
}

/// Compression context for a single request.
///
/// For streaming RPCs, contains per-envelope compression settings.
/// For unary RPCs, envelope compression is `None` (Tower handles HTTP body compression).
#[derive(Debug, Clone, Copy, Default)]
pub struct CompressionContext {
    /// Per-envelope compression for streaming RPCs (None for unary).
    pub envelope: Option<EnvelopeCompression>,
    /// Full compression configuration (includes level and min_bytes).
    pub config: CompressionConfig,
}

impl ConnectContext {
    /// Build request context from request headers and server config.
    ///
    /// Detects protocol, parses compression headers, and computes timeout.
    /// Returns error only for malformed headers (e.g., unsupported compression).
    ///
    /// Call [`validate`] after building to check protocol requirements.
    pub(crate) fn from_request<B>(
        req: &Request<B>,
        config: &ServerConfig,
    ) -> Result<Self, ContextError> {
        let protocol = detect_protocol(req);

        // Parse envelope compression for POST requests (streaming only, unary returns None)
        let compression = if *req.method() == Method::POST {
            let envelope = parse_envelope_compression(req, protocol.is_streaming())
                .map_err(|err| ContextError::new(protocol, err))?;
            CompressionContext {
                envelope,
                config: config.compression,
            }
        } else {
            CompressionContext::default()
        };

        let client_timeout = parse_timeout(req);
        let timeout = compute_effective_timeout(config.server_timeout, client_timeout);

        Ok(Self {
            protocol,
            compression,
            timeout,
            limits: config.limits,
            require_protocol_header: config.require_protocol_header,
        })
    }

    /// Validate protocol requirements for the request.
    ///
    /// Checks for POST requests:
    /// - Content-Type maps to a known protocol variant
    /// - Connect-Protocol-Version header is present (if required by config)
    ///
    /// Checks for GET requests:
    /// - `encoding` parameter is present and valid (json or proto)
    /// - `message` parameter is present
    /// - `connect` parameter is "v1" if present, or required when config requires it
    /// - `compression` parameter if present, is a supported algorithm
    ///
    /// Returns `Ok(())` if valid, or `Err(ResponseError)` if validation fails.
    pub fn validate<B>(&self, req: &Request<B>) -> Result<(), ContextError> {
        // GET request validation
        if *req.method() == Method::GET {
            if let Some(err) = validate_get_query_params(req, self.require_protocol_header) {
                return Err(ContextError::new(self.protocol, err));
            }
            return Ok(());
        }

        // POST request validation
        if *req.method() != Method::POST {
            return Ok(());
        }

        // Check content-type is known
        if let Some(err) = validate_content_type(self.protocol) {
            return Err(ContextError::new(self.protocol, err));
        }

        // Check protocol version header
        if let Some(err) = validate_protocol_version(req, self.require_protocol_header) {
            return Err(ContextError::new(self.protocol, err));
        }

        Ok(())
    }
}
