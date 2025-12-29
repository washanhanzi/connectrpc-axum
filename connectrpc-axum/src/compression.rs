//! Compression support for Connect unary RPCs.
//!
//! Uses standard HTTP headers: `Content-Encoding` / `Accept-Encoding`.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression as GzipLevel;
use std::io::{self, Read, Write};

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
pub fn negotiate_response_encoding(accept: Option<&str>) -> CompressionEncoding {
    // Simple: if gzip is in Accept-Encoding, use it
    match accept {
        Some(s) if s.contains("gzip") => CompressionEncoding::Gzip,
        _ => CompressionEncoding::Identity,
    }
}

pub fn compress(bytes: &[u8], encoding: CompressionEncoding) -> io::Result<Vec<u8>> {
    match encoding {
        CompressionEncoding::Identity => Ok(bytes.to_vec()),
        CompressionEncoding::Gzip => {
            let mut encoder = GzEncoder::new(Vec::new(), GzipLevel::default());
            encoder.write_all(bytes)?;
            encoder.finish()
        }
    }
}

pub fn decompress(bytes: &[u8], encoding: CompressionEncoding) -> io::Result<Vec<u8>> {
    match encoding {
        CompressionEncoding::Identity => Ok(bytes.to_vec()),
        CompressionEncoding::Gzip => {
            let mut decoder = GzDecoder::new(bytes);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            Ok(decompressed)
        }
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
    fn test_compress_decompress_gzip() {
        let original = b"Hello, World! This is a test message.";
        let compressed = compress(original, CompressionEncoding::Gzip).unwrap();

        // Compressed should be different from original
        assert_ne!(compressed.as_slice(), original);

        // Decompress should give back original
        let decompressed = decompress(&compressed, CompressionEncoding::Gzip).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_decompress_identity() {
        let original = b"Hello, World!";
        let compressed = compress(original, CompressionEncoding::Identity).unwrap();
        assert_eq!(compressed, original);

        let decompressed = decompress(&compressed, CompressionEncoding::Identity).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_decompress_invalid_gzip() {
        let invalid = b"not valid gzip data";
        let result = decompress(invalid, CompressionEncoding::Gzip);
        assert!(result.is_err());
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
