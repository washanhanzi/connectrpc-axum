//! Compression support for Connect unary RPCs.
//!
//! Uses standard HTTP headers: `Content-Encoding` / `Accept-Encoding`.
//!
//! ## Architecture
//!
//! Compression is handled via the [`Codec`] trait, which defines a standard interface
//! for compression/decompression. Built-in codecs include:
//! - [`IdentityCodec`]: No-op codec (zero-copy passthrough)
//! - [`GzipCodec`]: Gzip compression via flate2
//!
//! For custom compression algorithms (zstd, brotli, etc.), implement the [`Codec`] trait.

use crate::error::{Code, ConnectError};
use axum::http::header::{ACCEPT_ENCODING, CONTENT_ENCODING};
use axum::http::Request;
use bytes::Bytes;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression as GzipLevel;
use std::io::{self, Read, Write};
use std::sync::Arc;

/// Supported compression encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionEncoding {
    #[default]
    Identity,
    Gzip,
}

impl CompressionEncoding {
    /// Parse from Content-Encoding header value.
    /// Returns None for unsupported encodings (caller should return Unimplemented).
    pub fn from_header(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("identity") | Some("") => Some(Self::Identity),
            Some("gzip") => Some(Self::Gzip),
            _ => None, // unsupported
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
        }
    }
}

// ============================================================================
// Codec Trait and Implementations
// ============================================================================

/// Trait for compression/decompression codecs.
///
/// Implement this trait to add custom compression algorithms.
/// Built-in implementations: [`IdentityCodec`], [`GzipCodec`].
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum::compression::Codec;
/// use bytes::Bytes;
/// use std::io;
///
/// struct ZstdCodec { level: i32 }
///
/// impl Codec for ZstdCodec {
///     fn name(&self) -> &'static str { "zstd" }
///
///     fn compress(&self, data: Bytes) -> io::Result<Bytes> {
///         // ... zstd compression
///     }
///
///     fn decompress(&self, data: Bytes) -> io::Result<Bytes> {
///         // ... zstd decompression
///     }
/// }
/// ```
pub trait Codec: Send + Sync + 'static {
    /// The encoding name for HTTP headers (e.g., "gzip", "zstd", "br").
    fn name(&self) -> &'static str;

    /// Compress data.
    ///
    /// Takes ownership of input to enable zero-copy for identity codec.
    fn compress(&self, data: Bytes) -> io::Result<Bytes>;

    /// Decompress data.
    ///
    /// Takes ownership of input to enable zero-copy for identity codec.
    fn decompress(&self, data: Bytes) -> io::Result<Bytes>;
}

/// Identity codec - zero-copy passthrough (no compression).
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityCodec;

impl Codec for IdentityCodec {
    fn name(&self) -> &'static str {
        "identity"
    }

    fn compress(&self, data: Bytes) -> io::Result<Bytes> {
        Ok(data) // zero-copy
    }

    fn decompress(&self, data: Bytes) -> io::Result<Bytes> {
        Ok(data) // zero-copy
    }
}

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
        Self { level: level.min(9) }
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
        let mut decoder = GzDecoder::new(&data[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(Bytes::from(decompressed))
    }
}

/// Get the default codec for a compression encoding.
///
/// Returns a static reference to avoid allocation for built-in codecs.
pub fn default_codec(encoding: CompressionEncoding) -> Arc<dyn Codec> {
    match encoding {
        CompressionEncoding::Identity => Arc::new(IdentityCodec),
        CompressionEncoding::Gzip => Arc::new(GzipCodec::default()),
    }
}

/// Compression context stored in request extensions.
/// Set by ConnectService, used by request extractors and response serialization.
#[derive(Debug, Clone, Copy, Default)]
pub struct Compression {
    /// Encoding used for the request body (from Content-Encoding header).
    pub request: CompressionEncoding,
    /// Negotiated encoding for the response (from Accept-Encoding header).
    pub response: CompressionEncoding,
}

/// Server compression configuration.
#[derive(Debug, Clone, Copy)]
pub struct CompressionConfig {
    /// Minimum bytes before compression is applied (default: 1024).
    /// Messages smaller than this threshold are sent uncompressed.
    pub min_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self { min_bytes: 1024 }
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
                let q = params
                    .split(';')
                    .find_map(|p| p.trim().strip_prefix("q="));
                (enc.trim(), q)
            }
            None => (token, None),
        };

        // Skip if q=0 (explicitly disabled)
        if let Some(q) = q_value {
            if q.trim() == "0" || q.trim() == "0.0" || q.trim() == "0.00" || q.trim() == "0.000" {
                continue;
            }
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

/// Header name for Connect streaming request compression.
const CONNECT_CONTENT_ENCODING: &str = "connect-content-encoding";

/// Header name for Connect streaming response compression negotiation.
const CONNECT_ACCEPT_ENCODING: &str = "connect-accept-encoding";

/// Parse compression settings from request headers.
///
/// For Connect streaming protocol:
/// - Uses `Connect-Content-Encoding` for request body compression
/// - Uses `Connect-Accept-Encoding` for response compression negotiation
///
/// For unary protocol:
/// - Uses standard `Content-Encoding` for request body compression
/// - Uses standard `Accept-Encoding` for response compression negotiation
///
/// Returns `Err(ConnectError)` if the Content-Encoding is unsupported.
pub fn parse_compression<B>(
    req: &Request<B>,
    is_streaming: bool,
) -> Result<Compression, ConnectError> {
    // Connect streaming uses Connect-Content-Encoding, unary uses Content-Encoding
    let content_encoding = if is_streaming {
        req.headers()
            .get(CONNECT_CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
    } else {
        req.headers()
            .get(CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
    };

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

    // Connect streaming uses Connect-Accept-Encoding, unary uses Accept-Encoding
    let accept_encoding = if is_streaming {
        req.headers()
            .get(CONNECT_ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
    } else {
        req.headers()
            .get(ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
    };
    let response_encoding = negotiate_response_encoding(accept_encoding);

    Ok(Compression {
        request: request_encoding,
        response: response_encoding,
    })
}

/// Compress bytes using the specified encoding.
///
/// Uses the default codec for the encoding. For custom codecs with
/// specific configuration, use the [`Codec`] trait directly.
pub fn compress(bytes: Bytes, encoding: CompressionEncoding) -> io::Result<Bytes> {
    match encoding {
        CompressionEncoding::Identity => Ok(bytes), // zero-copy
        CompressionEncoding::Gzip => GzipCodec::default().compress(bytes),
    }
}

/// Decompress bytes using the specified encoding.
///
/// Uses the default codec for the encoding. For custom codecs with
/// specific configuration, use the [`Codec`] trait directly.
pub fn decompress(bytes: Bytes, encoding: CompressionEncoding) -> io::Result<Bytes> {
    match encoding {
        CompressionEncoding::Identity => Ok(bytes), // zero-copy
        CompressionEncoding::Gzip => GzipCodec::default().decompress(bytes),
    }
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
    fn test_compress_decompress_gzip() {
        let original = Bytes::from_static(b"Hello, World! This is a test message.");
        let compressed = compress(original.clone(), CompressionEncoding::Gzip).unwrap();

        // Compressed should be different from original
        assert_ne!(compressed, original);

        // Decompress should give back original
        let decompressed = decompress(compressed, CompressionEncoding::Gzip).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_decompress_identity() {
        let original = Bytes::from_static(b"Hello, World!");
        let compressed = compress(original.clone(), CompressionEncoding::Identity).unwrap();
        assert_eq!(compressed, original);

        let decompressed = decompress(compressed, CompressionEncoding::Identity).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_decompress_invalid_gzip() {
        let invalid = Bytes::from_static(b"not valid gzip data");
        let result = decompress(invalid, CompressionEncoding::Gzip);
        assert!(result.is_err());
    }

    #[test]
    fn test_codec_trait_gzip() {
        let codec = GzipCodec::default();
        assert_eq!(codec.name(), "gzip");

        let original = Bytes::from_static(b"Hello, World! This is a test message.");
        let compressed = codec.compress(original.clone()).unwrap();
        assert_ne!(compressed, original);

        let decompressed = codec.decompress(compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_codec_trait_identity() {
        let codec = IdentityCodec;
        assert_eq!(codec.name(), "identity");

        let original = Bytes::from_static(b"Hello, World!");
        let compressed = codec.compress(original.clone()).unwrap();
        assert_eq!(compressed, original);

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
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.min_bytes, 1024);
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
}
