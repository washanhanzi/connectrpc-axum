//! Context and common types for Connect RPC request handling.
//!
//! This module provides types used by the [`ConnectLayer`] middleware
//! and request extensions, including protocol detection, timeout configuration,
//! compression, and message size limits.
//!
//! [`ConnectLayer`]: crate::layer::ConnectLayer

pub mod compression;
pub mod config;
pub mod error;
pub mod limit;
pub mod protocol;
pub mod timeout;

use axum::http::{Method, Request};
use std::time::Duration;

// Re-export compression types and functions
pub use compression::{
    Codec, Compression, CompressionConfig, CompressionEncoding, GzipCodec, IdentityCodec, compress,
    decompress, default_codec, negotiate_response_encoding, parse_compression,
};

// Re-export config types
pub use config::ServerConfig;

// Re-export error types
pub use error::ContextError;

// Re-export limit types
pub use limit::{DEFAULT_MAX_MESSAGE_SIZE, MessageLimits};

// Re-export protocol types and functions
pub use protocol::{
    CONNECT_PROTOCOL_VERSION, CONNECT_PROTOCOL_VERSION_HEADER, RequestProtocol, detect_protocol,
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
#[derive(Debug, Clone, Copy, Default)]
pub struct CompressionContext {
    /// Encoding of incoming request body (from Content-Encoding header)
    pub request_encoding: CompressionEncoding,
    /// Negotiated encoding for response (from Accept-Encoding header)
    pub response_encoding: CompressionEncoding,
    /// Minimum bytes before compression is applied
    pub min_compress_bytes: usize,
}

impl CompressionContext {
    /// Create a new compression context.
    pub fn new(
        request_encoding: CompressionEncoding,
        response_encoding: CompressionEncoding,
        min_compress_bytes: usize,
    ) -> Self {
        Self {
            request_encoding,
            response_encoding,
            min_compress_bytes,
        }
    }
}

impl ConnectContext {
    /// Build request context from request headers and server config.
    ///
    /// Detects protocol, parses compression headers, and computes timeout.
    /// Returns error only for malformed headers (e.g., unsupported compression).
    ///
    /// Call [`validate`] after building to check protocol requirements.
    pub fn from_request<B>(req: &Request<B>, config: &ServerConfig) -> Result<Self, ContextError> {
        // Detect protocol from Content-Type or query params
        let protocol = detect_protocol(req);

        // Parse compression for all POST requests (unary and streaming)
        let compression = if *req.method() == Method::POST {
            match parse_compression(req, protocol.is_streaming()) {
                Ok(c) => {
                    CompressionContext::new(c.request, c.response, config.compression.min_bytes)
                }
                Err(err) => return Err(ContextError::new(protocol, err)),
            }
        } else {
            CompressionContext::default()
        };

        // Compute effective timeout
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
