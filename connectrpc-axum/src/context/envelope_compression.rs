//! Envelope compression for Connect streaming RPCs.
//!
//! This module re-exports compression types from `connectrpc-axum-core` and provides
//! server-specific functionality for parsing compression headers from requests.
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

use crate::message::error::{Code, ConnectError};

// Re-export core types
pub use connectrpc_axum_core::{
    // Codec trait and implementations
    BoxedCodec, Codec, IdentityCodec,
    // Compression types
    CompressionConfig, CompressionEncoding, CompressionLevel,
    // Header constants
    CONNECT_ACCEPT_ENCODING, CONNECT_CONTENT_ENCODING,
    // Envelope functions
    compress_payload, decompress_bytes, compress_bytes,
    // Helper
    negotiate_response_encoding, supported_encodings_str,
};

/// Convert core CompressionLevel to tower_http CompressionLevel.
///
/// This is needed because tower-http's CompressionLayer uses its own type.
#[cfg(any(
    feature = "compression-gzip-unary",
    feature = "compression-deflate-unary",
    feature = "compression-br-unary",
    feature = "compression-zstd-unary"
))]
pub fn to_tower_compression_level(level: CompressionLevel) -> tower_http::CompressionLevel {
    match level {
        CompressionLevel::Fastest => tower_http::CompressionLevel::Fastest,
        CompressionLevel::Best => tower_http::CompressionLevel::Best,
        CompressionLevel::Default => tower_http::CompressionLevel::Default,
        CompressionLevel::Precise(n) => tower_http::CompressionLevel::Precise(n as i32),
    }
}

#[cfg(feature = "compression-gzip-stream")]
pub use connectrpc_axum_core::GzipCodec;

#[cfg(feature = "compression-deflate-stream")]
pub use connectrpc_axum_core::DeflateCodec;

#[cfg(feature = "compression-br-stream")]
pub use connectrpc_axum_core::BrotliCodec;

#[cfg(feature = "compression-zstd-stream")]
pub use connectrpc_axum_core::ZstdCodec;

// ============================================================================
// Server-specific types
// ============================================================================

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
                    "unsupported compression \"{}\": supported encodings are {}",
                    content_encoding.unwrap_or(""),
                    supported_encodings_str()
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

/// Resolve a codec from an encoding name.
///
/// Returns `None` for identity (no compression needed).
/// Returns `Some(BoxedCodec)` for supported encodings.
/// Returns `Err` for unsupported encodings.
pub fn resolve_codec(name: &str) -> Result<Option<BoxedCodec>, ConnectError> {
    match name {
        "" | "identity" => Ok(None),
        #[cfg(feature = "compression-gzip-stream")]
        "gzip" => Ok(Some(BoxedCodec::new(GzipCodec::default()))),
        #[cfg(feature = "compression-deflate-stream")]
        "deflate" => Ok(Some(BoxedCodec::new(DeflateCodec::default()))),
        #[cfg(feature = "compression-br-stream")]
        "br" => Ok(Some(BoxedCodec::new(BrotliCodec::default()))),
        #[cfg(feature = "compression-zstd-stream")]
        "zstd" => Ok(Some(BoxedCodec::new(ZstdCodec::default()))),
        other => Err(ConnectError::new(
            Code::Unimplemented,
            format!(
                "unsupported compression \"{}\": supported encodings are {}",
                other,
                supported_encodings_str()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_encoding_from_header_identity() {
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

        // Always unsupported
        assert_eq!(CompressionEncoding::from_header(Some("lz4")), None);
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_compression_encoding_from_header_gzip() {
        assert_eq!(
            CompressionEncoding::from_header(Some("gzip")),
            Some(CompressionEncoding::Gzip)
        );
    }

    #[test]
    fn test_compression_encoding_as_str_identity() {
        assert_eq!(CompressionEncoding::Identity.as_str(), "identity");
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_compression_encoding_as_str_gzip() {
        assert_eq!(CompressionEncoding::Gzip.as_str(), "gzip");
    }

    #[cfg(feature = "compression-gzip-stream")]
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
    fn test_negotiate_response_encoding_identity() {
        assert_eq!(
            negotiate_response_encoding(None),
            CompressionEncoding::Identity
        );
        assert_eq!(
            negotiate_response_encoding(Some("")),
            CompressionEncoding::Identity
        );
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_negotiate_response_encoding_gzip() {
        assert_eq!(
            negotiate_response_encoding(Some("gzip")),
            CompressionEncoding::Gzip
        );
    }

    #[cfg(feature = "compression-gzip-stream")]
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

        // Non-zero q values should be accepted
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=1")),
            CompressionEncoding::Gzip
        );
        assert_eq!(
            negotiate_response_encoding(Some("gzip;q=0.5")),
            CompressionEncoding::Gzip
        );
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_gzip_codec_compress_decompress() {
        let codec = GzipCodec::default();
        assert_eq!(codec.name(), "gzip");

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        assert_ne!(&compressed[..], &original[..]);

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[test]
    fn test_resolve_codec_identity() {
        // Identity
        assert!(resolve_codec("").unwrap().is_none());
        assert!(resolve_codec("identity").unwrap().is_none());

        // Always unsupported
        assert!(resolve_codec("lz4").is_err());
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_resolve_codec_gzip() {
        let codec = resolve_codec("gzip").unwrap();
        assert!(codec.is_some());
        assert_eq!(codec.unwrap().name(), "gzip");
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

    #[cfg(feature = "compression-br-stream")]
    #[test]
    fn test_brotli_codec_compress_decompress() {
        let codec = BrotliCodec::default();
        assert_eq!(codec.name(), "br");

        let original = b"Hello, World! This is a test message for brotli.";
        let compressed = codec.compress(original).unwrap();
        assert_ne!(&compressed[..], &original[..]);

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[cfg(feature = "compression-zstd-stream")]
    #[test]
    fn test_zstd_codec_compress_decompress() {
        let codec = ZstdCodec::default();
        assert_eq!(codec.name(), "zstd");

        let original = b"Hello, World! This is a test message for zstd.";
        let compressed = codec.compress(original).unwrap();
        assert_ne!(&compressed[..], &original[..]);

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }
}
