//! Envelope compression for Connect streaming RPCs.
//!
//! This module handles per-envelope compression in streaming Connect RPCs.
//! HTTP body compression (for unary RPCs) is handled by Tower middleware.
//!
//! ## Architecture
//!
//! The Connect protocol uses two different compression mechanisms:
//!
//! - **Unary RPCs**: Use standard HTTP `Content-Encoding`/`Accept-Encoding` headers.
//!   This is handled by Tower's `CompressionLayer`.
//!
//! - **Streaming RPCs**: Use `Connect-Content-Encoding`/`Connect-Accept-Encoding` headers.
//!   Each message envelope is individually compressed. This module handles that.
//!
//! ## Codec Trait
//!
//! The [`Codec`] trait provides a simple `Bytes → Bytes` API for compression.
//! This is intentionally simple because the envelope format requires full buffering anyway:
//!
//! ```text
//! [flags:1][length:4][payload]
//! ```
//!
//! We must read all `length` bytes before decompression, so streaming codecs
//! provide no benefit for envelope compression.

use crate::error::{Code, ConnectError};
use bytes::Bytes;
use flate2::Compression as GzipLevel;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{self, Read, Write};
use std::sync::Arc;

// ============================================================================
// Codec Trait
// ============================================================================

/// Codec trait for per-message (envelope) compression.
///
/// Used for streaming Connect RPCs where each message is individually compressed.
/// HTTP body compression for unary RPCs is handled by Tower middleware.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum::context::message_compression::Codec;
/// use bytes::Bytes;
/// use std::io;
///
/// struct Lz4Codec;
///
/// impl Codec for Lz4Codec {
///     fn name(&self) -> &'static str { "lz4" }
///
///     fn compress(&self, data: Bytes) -> io::Result<Bytes> {
///         // ... lz4 compression
///     }
///
///     fn decompress(&self, data: Bytes) -> io::Result<Bytes> {
///         // ... lz4 decompression
///     }
/// }
/// ```
pub trait Codec: Send + Sync + 'static {
    /// The encoding name for HTTP headers (e.g., "gzip", "zstd", "br").
    fn name(&self) -> &'static str;

    /// Compress data.
    fn compress(&self, data: Bytes) -> io::Result<Bytes>;

    /// Decompress data.
    fn decompress(&self, data: Bytes) -> io::Result<Bytes>;
}

// ============================================================================
// Boxed Codec
// ============================================================================

/// A boxed codec for type-erased storage.
///
/// Use `Option<BoxedCodec>` where `None` represents identity (no compression).
#[derive(Clone)]
pub struct BoxedCodec(Arc<dyn Codec>);

impl BoxedCodec {
    /// Create a new boxed codec.
    pub fn new<C: Codec>(codec: C) -> Self {
        BoxedCodec(Arc::new(codec))
    }

    /// Get the codec name for HTTP headers.
    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    /// Compress data.
    pub fn compress(&self, data: Bytes) -> io::Result<Bytes> {
        self.0.compress(data)
    }

    /// Decompress data.
    pub fn decompress(&self, data: Bytes) -> io::Result<Bytes> {
        self.0.decompress(data)
    }
}

impl std::fmt::Debug for BoxedCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BoxedCodec").field(&self.name()).finish()
    }
}

// ============================================================================
// Built-in Codecs
// ============================================================================

/// Gzip codec using flate2.
#[derive(Debug, Clone, Copy)]
pub struct GzipCodec {
    /// Compression level (0-9). Default is 6.
    pub level: u32,
}

impl Default for GzipCodec {
    fn default() -> Self {
        Self { level: 6 }
    }
}

impl GzipCodec {
    /// Create a new GzipCodec with the specified compression level.
    ///
    /// Level ranges from 0 (no compression) to 9 (best compression).
    pub fn with_level(level: u32) -> Self {
        Self {
            level: level.min(9),
        }
    }
}

impl Codec for GzipCodec {
    fn name(&self) -> &'static str {
        "gzip"
    }

    fn compress(&self, data: Bytes) -> io::Result<Bytes> {
        let mut encoder = GzEncoder::new(Vec::new(), GzipLevel::new(self.level));
        encoder.write_all(&data)?;
        Ok(Bytes::from(encoder.finish()?))
    }

    fn decompress(&self, data: Bytes) -> io::Result<Bytes> {
        decompress_gzip(data)
    }
}

/// Decompress gzip data.
///
/// Size limits are enforced at the HTTP layer (BridgeLayer checks Content-Length)
/// before decompression occurs.
pub(crate) fn decompress_gzip(data: Bytes) -> io::Result<Bytes> {
    let mut decoder = GzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(Bytes::from(decompressed))
}

// ============================================================================
// Codec Resolution
// ============================================================================

/// Resolve a codec from an encoding name.
///
/// Returns `None` for identity (no compression needed).
/// Returns `Some(BoxedCodec)` for supported encodings.
/// Returns `Err` for unsupported encodings.
pub fn resolve_codec(name: &str) -> Result<Option<BoxedCodec>, ConnectError> {
    match name {
        "" | "identity" => Ok(None),
        "gzip" => Ok(Some(BoxedCodec::new(GzipCodec::default()))),
        other => Err(ConnectError::new(
            Code::Unimplemented,
            format!(
                "unsupported compression \"{}\": supported encodings are gzip, identity",
                other
            ),
        )),
    }
}

// ============================================================================
// Compression Helpers
// ============================================================================

/// Compress bytes using the specified codec.
///
/// If `codec` is `None`, returns the input unchanged (identity).
pub fn compress_bytes(bytes: Bytes, codec: Option<&BoxedCodec>) -> io::Result<Bytes> {
    match codec {
        None => Ok(bytes), // identity: zero-copy passthrough
        Some(c) => c.compress(bytes),
    }
}

/// Decompress bytes using the specified codec.
///
/// If `codec` is `None`, returns the input unchanged (identity).
pub fn decompress_bytes(bytes: Bytes, codec: Option<&BoxedCodec>) -> io::Result<Bytes> {
    match codec {
        None => Ok(bytes), // identity: zero-copy passthrough
        Some(c) => c.decompress(bytes),
    }
}

// ============================================================================
// Compression Encoding Enum
// ============================================================================

/// Supported compression encodings.
///
/// This enum is used for header parsing and negotiation.
/// Use [`resolve_codec`] to get the actual codec implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionEncoding {
    #[default]
    Identity,
    Gzip,
}

impl CompressionEncoding {
    /// Parse from Content-Encoding or Connect-Content-Encoding header value.
    /// Returns None for unsupported encodings (caller should return Unimplemented).
    pub fn from_header(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("identity") | Some("") => Some(Self::Identity),
            Some("gzip") => Some(Self::Gzip),
            _ => None, // unsupported
        }
    }

    /// Get the header value string for this encoding.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
        }
    }

    /// Get the codec for this encoding.
    ///
    /// Returns `None` for identity, `Some(BoxedCodec)` for others.
    pub fn codec(&self) -> Option<BoxedCodec> {
        match self {
            Self::Identity => None,
            Self::Gzip => Some(BoxedCodec::new(GzipCodec::default())),
        }
    }
}

// ============================================================================
// Compression Configuration
// ============================================================================

/// Server compression configuration.
#[derive(Debug, Clone, Copy)]
pub struct CompressionConfig {
    /// Minimum bytes before compression is applied.
    /// Default is 0 (compress everything), matching connect-go behavior.
    /// Messages smaller than this threshold are sent uncompressed.
    pub min_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        // Connect-go default is 0 (compress everything)
        Self { min_bytes: 0 }
    }
}

impl CompressionConfig {
    /// Create a new compression config with the specified minimum bytes threshold.
    pub fn new(min_bytes: usize) -> Self {
        Self { min_bytes }
    }

    /// Disable compression by setting threshold to usize::MAX.
    pub fn disabled() -> Self {
        Self {
            min_bytes: usize::MAX,
        }
    }
}

// ============================================================================
// Header Parsing
// ============================================================================

/// Header name for Connect streaming request compression.
pub const CONNECT_CONTENT_ENCODING: &str = "connect-content-encoding";

/// Header name for Connect streaming response compression negotiation.
pub const CONNECT_ACCEPT_ENCODING: &str = "connect-accept-encoding";

/// Negotiate response encoding from Accept-Encoding header.
///
/// Follows connect-go's approach: first supported encoding wins (client preference order).
/// Respects `q=0` which means "not acceptable" per RFC 7231.
pub fn negotiate_response_encoding(accept: Option<&str>) -> CompressionEncoding {
    let Some(accept) = accept else {
        return CompressionEncoding::Identity;
    };

    for token in accept.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        // Parse "gzip;q=0.5" into encoding="gzip", q_value=Some("0.5")
        let (encoding, q_value) = match token.split_once(';') {
            Some((enc, params)) => {
                let q = params.split(';').find_map(|p| p.trim().strip_prefix("q="));
                (enc.trim(), q)
            }
            None => (token, None),
        };

        // Skip if q=0 (explicitly disabled)
        if let Some(q) = q_value
            && (q.trim() == "0" || q.trim() == "0.0" || q.trim() == "0.00" || q.trim() == "0.000")
        {
            continue;
        }

        // Return first supported encoding
        match encoding {
            "gzip" => return CompressionEncoding::Gzip,
            "identity" => return CompressionEncoding::Identity,
            _ => continue,
        }
    }

    CompressionEncoding::Identity
}

/// Per-envelope compression settings for streaming RPCs.
///
/// Parsed from `Connect-Content-Encoding` and `Connect-Accept-Encoding` headers.
/// Only used for streaming - unary RPCs use Tower's HTTP body compression.
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeCompression {
    /// Encoding used for request envelopes (from `Connect-Content-Encoding`).
    pub request: CompressionEncoding,
    /// Negotiated encoding for response envelopes (from `Connect-Accept-Encoding`).
    pub response: CompressionEncoding,
}

/// Parse envelope compression settings from streaming request headers.
///
/// Returns `Some(EnvelopeCompression)` for streaming RPCs, parsing:
/// - `Connect-Content-Encoding` for per-envelope request compression
/// - `Connect-Accept-Encoding` for per-envelope response compression negotiation
///
/// Returns `None` for unary RPCs (Tower handles HTTP body compression via
/// `Content-Encoding`/`Accept-Encoding`).
///
/// Returns `Err(ConnectError)` if `Connect-Content-Encoding` is unsupported.
pub fn parse_envelope_compression<B>(
    req: &axum::http::Request<B>,
    is_streaming: bool,
) -> Result<Option<EnvelopeCompression>, ConnectError> {
    // Unary compression is handled by Tower's CompressionLayer/DecompressionLayer
    // via Content-Encoding/Accept-Encoding headers.
    if !is_streaming {
        return Ok(None);
    }

    // Parse Connect-Content-Encoding for streaming request compression
    let content_encoding = req
        .headers()
        .get(CONNECT_CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());

    let request_encoding = match CompressionEncoding::from_header(content_encoding) {
        Some(enc) => enc,
        None => {
            return Err(ConnectError::new(
                Code::Unimplemented,
                format!(
                    "unsupported compression \"{}\": supported encodings are gzip, identity",
                    content_encoding.unwrap_or("")
                ),
            ));
        }
    };

    // Parse Connect-Accept-Encoding for streaming response compression
    let accept_encoding = req
        .headers()
        .get(CONNECT_ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok());
    let response_encoding = negotiate_response_encoding(accept_encoding);

    Ok(Some(EnvelopeCompression {
        request: request_encoding,
        response: response_encoding,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_encoding_from_header() {
        // Identity cases
        assert_eq!(
            CompressionEncoding::from_header(None),
            Some(CompressionEncoding::Identity)
        );
        assert_eq!(
            CompressionEncoding::from_header(Some("")),
            Some(CompressionEncoding::Identity)
        );
        assert_eq!(
            CompressionEncoding::from_header(Some("identity")),
            Some(CompressionEncoding::Identity)
        );

        // Gzip
        assert_eq!(
            CompressionEncoding::from_header(Some("gzip")),
            Some(CompressionEncoding::Gzip)
        );

        // Unsupported
        assert_eq!(CompressionEncoding::from_header(Some("br")), None);
        assert_eq!(CompressionEncoding::from_header(Some("deflate")), None);
        assert_eq!(CompressionEncoding::from_header(Some("zstd")), None);
    }

    #[test]
    fn test_compression_encoding_as_str() {
        assert_eq!(CompressionEncoding::Identity.as_str(), "identity");
        assert_eq!(CompressionEncoding::Gzip.as_str(), "gzip");
    }

    #[test]
    fn test_compression_encoding_codec() {
        // Identity returns None
        assert!(CompressionEncoding::Identity.codec().is_none());

        // Gzip returns Some
        let codec = CompressionEncoding::Gzip.codec();
        assert!(codec.is_some());
        assert_eq!(codec.unwrap().name(), "gzip");
    }

    #[test]
    fn test_negotiate_response_encoding() {
        // Gzip requested
        assert_eq!(
            negotiate_response_encoding(Some("gzip")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip, deflate, br")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("deflate, gzip")),
            CompressionEncoding::Gzip
        );

        // No gzip
        assert_eq!(
            negotiate_response_encoding(Some("deflate, br")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(None),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("")),
            CompressionEncoding::Identity
        );
    }

    #[test]
    fn test_negotiate_response_encoding_order() {
        // First supported encoding wins (client preference order)
        assert_eq!(
            negotiate_response_encoding(Some("br, gzip")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip, identity")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("identity, gzip")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("br, zstd, gzip")),
            CompressionEncoding::Gzip
        );
    }

    #[test]
    fn test_negotiate_response_encoding_q_values() {
        // q=0 means "not acceptable" - should be skipped
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0, identity")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0.0")),
            CompressionEncoding::Identity
        );

        // Non-zero q values should be accepted (we ignore the actual weight)
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=1")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0.5")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0.001")),
            CompressionEncoding::Gzip
        );

        // Mixed: skip disabled, use first enabled
        assert_eq!(
            negotiate_response_encoding(Some("br;q=1, gzip;q=0, identity")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0, identity;q=0")),
            CompressionEncoding::Identity
        );
    }

    #[test]
    fn test_negotiate_response_encoding_whitespace() {
        // Handle various whitespace scenarios
        assert_eq!(
            negotiate_response_encoding(Some("  gzip  ")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip ; q=0")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;  q=0")),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("br ,  gzip")),
            CompressionEncoding::Gzip
        );
    }

    #[test]
    fn test_gzip_codec_compress_decompress() {
        let codec = GzipCodec::default();
        assert_eq!(codec.name(), "gzip");

        let original = Bytes::from_static(b"Hello, World! This is a test message.");
        let compressed = codec.compress(original.clone()).unwrap();
        assert_ne!(compressed, original);

        let decompressed = codec.decompress(compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_gzip_codec_with_level() {
        let codec = GzipCodec::with_level(9);
        assert_eq!(codec.level, 9);

        let original = Bytes::from_static(b"Hello, World! This is a test message.");
        let compressed = codec.compress(original.clone()).unwrap();
        let decompressed = codec.decompress(compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_boxed_codec() {
        let codec = BoxedCodec::new(GzipCodec::default());
        assert_eq!(codec.name(), "gzip");

        let original = Bytes::from_static(b"Hello, World! This is a test message.");
        let compressed = codec.compress(original.clone()).unwrap();
        assert_ne!(compressed, original);

        let decompressed = codec.decompress(compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_decompress_bytes_with_codec() {
        let codec = BoxedCodec::new(GzipCodec::default());
        let original = Bytes::from_static(b"Hello, World! This is a test message.");

        let compressed = compress_bytes(original.clone(), Some(&codec)).unwrap();
        assert_ne!(compressed, original);

        let decompressed = decompress_bytes(compressed, Some(&codec)).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_decompress_bytes_identity() {
        let original = Bytes::from_static(b"Hello, World!");

        // None = identity, zero-copy passthrough
        let compressed = compress_bytes(original.clone(), None).unwrap();
        assert_eq!(compressed, original);

        let decompressed = decompress_bytes(compressed, None).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_resolve_codec() {
        // Identity
        assert!(resolve_codec("").unwrap().is_none());
        assert!(resolve_codec("identity").unwrap().is_none());

        // Gzip
        let codec = resolve_codec("gzip").unwrap();
        assert!(codec.is_some());
        assert_eq!(codec.unwrap().name(), "gzip");

        // Unsupported
        assert!(resolve_codec("br").is_err());
        assert!(resolve_codec("zstd").is_err());
    }

    #[test]
    fn test_decompress_invalid_gzip() {
        let codec = BoxedCodec::new(GzipCodec::default());
        let invalid = Bytes::from_static(b"not valid gzip data");
        let result = codec.decompress(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        // Connect-go default is 0 (compress everything)
        assert_eq!(config.min_bytes, 0);
    }

    #[test]
    fn test_compression_config_new() {
        let config = CompressionConfig::new(512);
        assert_eq!(config.min_bytes, 512);
    }

    #[test]
    fn test_compression_config_disabled() {
        let config = CompressionConfig::disabled();
        assert_eq!(config.min_bytes, usize::MAX);
    }

    #[test]
    fn test_boxed_codec_debug() {
        let codec = BoxedCodec::new(GzipCodec::default());
        let debug_str = format!("{:?}", codec);
        assert!(debug_str.contains("BoxedCodec"));
        assert!(debug_str.contains("gzip"));
    }
}
