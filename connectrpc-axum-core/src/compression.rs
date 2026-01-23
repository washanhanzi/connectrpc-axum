//! Compression configuration types.
//!
//! This module provides configuration types for compression in ConnectRPC:
//! - [`CompressionEncoding`]: Supported compression algorithms
//! - [`CompressionLevel`]: Compression quality settings
//! - [`CompressionConfig`]: Server/client compression configuration

use crate::codec::BoxedCodec;

#[cfg(feature = "compression-gzip-stream")]
use crate::codec::GzipCodec;

#[cfg(feature = "compression-deflate-stream")]
use crate::codec::DeflateCodec;

#[cfg(feature = "compression-br-stream")]
use crate::codec::BrotliCodec;

#[cfg(feature = "compression-zstd-stream")]
use crate::codec::ZstdCodec;

/// Supported compression encodings.
///
/// This enum is used for header parsing and negotiation.
/// Use [`CompressionEncoding::codec()`] to get the actual codec implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionEncoding {
    #[default]
    Identity,
    #[cfg(feature = "compression-gzip-stream")]
    Gzip,
    #[cfg(feature = "compression-deflate-stream")]
    Deflate,
    #[cfg(feature = "compression-br-stream")]
    Brotli,
    #[cfg(feature = "compression-zstd-stream")]
    Zstd,
}

impl CompressionEncoding {
    /// Parse from Content-Encoding or Connect-Content-Encoding header value.
    /// Returns None for unsupported encodings (caller should return Unimplemented).
    pub fn from_header(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("identity") | Some("") => Some(Self::Identity),
            #[cfg(feature = "compression-gzip-stream")]
            Some("gzip") => Some(Self::Gzip),
            #[cfg(feature = "compression-deflate-stream")]
            Some("deflate") => Some(Self::Deflate),
            #[cfg(feature = "compression-br-stream")]
            Some("br") => Some(Self::Brotli),
            #[cfg(feature = "compression-zstd-stream")]
            Some("zstd") => Some(Self::Zstd),
            _ => None, // unsupported
        }
    }

    /// Get the header value string for this encoding.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            #[cfg(feature = "compression-gzip-stream")]
            Self::Gzip => "gzip",
            #[cfg(feature = "compression-deflate-stream")]
            Self::Deflate => "deflate",
            #[cfg(feature = "compression-br-stream")]
            Self::Brotli => "br",
            #[cfg(feature = "compression-zstd-stream")]
            Self::Zstd => "zstd",
        }
    }

    /// Returns true if this encoding is identity (no compression).
    pub fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }

    /// Get the codec for this encoding.
    ///
    /// Returns `None` for identity, `Some(BoxedCodec)` for others.
    pub fn codec(&self) -> Option<BoxedCodec> {
        match self {
            Self::Identity => None,
            #[cfg(feature = "compression-gzip-stream")]
            Self::Gzip => Some(BoxedCodec::new(GzipCodec::default())),
            #[cfg(feature = "compression-deflate-stream")]
            Self::Deflate => Some(BoxedCodec::new(DeflateCodec::default())),
            #[cfg(feature = "compression-br-stream")]
            Self::Brotli => Some(BoxedCodec::new(BrotliCodec::default())),
            #[cfg(feature = "compression-zstd-stream")]
            Self::Zstd => Some(BoxedCodec::new(ZstdCodec::default())),
        }
    }

    /// Get the codec for this encoding with the specified compression level.
    ///
    /// Returns `None` for identity, `Some(BoxedCodec)` for others.
    #[allow(unused_variables)]
    pub fn codec_with_level(&self, level: CompressionLevel) -> Option<BoxedCodec> {
        match self {
            Self::Identity => None,
            #[cfg(feature = "compression-gzip-stream")]
            Self::Gzip => Some(BoxedCodec::new(GzipCodec::with_level(level_to_flate2(level)))),
            #[cfg(feature = "compression-deflate-stream")]
            Self::Deflate => Some(BoxedCodec::new(DeflateCodec::with_level(level_to_flate2(level)))),
            #[cfg(feature = "compression-br-stream")]
            Self::Brotli => Some(BoxedCodec::new(BrotliCodec::with_quality(level_to_brotli(
                level,
            )))),
            #[cfg(feature = "compression-zstd-stream")]
            Self::Zstd => Some(BoxedCodec::new(ZstdCodec::with_level(level_to_zstd(level)))),
        }
    }
}

/// Compression level configuration.
///
/// This is a local definition that doesn't depend on tower-http,
/// making it suitable for use in both client and server contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// Fastest compression (lowest ratio).
    Fastest,
    /// Best compression (highest ratio, slowest).
    Best,
    /// Default compression level for each algorithm.
    #[default]
    Default,
    /// Precise compression level (algorithm-specific value).
    Precise(u32),
}

impl CompressionLevel {
    /// Create a compression level with a precise value.
    ///
    /// The value interpretation is algorithm-specific:
    /// - gzip/deflate: 0-9 (0=no compression, 9=best)
    /// - brotli: 0-11 (0=fastest, 11=best)
    /// - zstd: 1-22 (1=fastest, 22=best)
    pub fn precise(level: u32) -> Self {
        CompressionLevel::Precise(level)
    }
}

/// Convert CompressionLevel to flate2 gzip level (0-9).
///
/// Matches tower-http → async_compression behavior:
/// - `Fastest` → 1
/// - `Best` → 9
/// - `Default` → 6
/// - `Precise(n)` → n clamped to 0-9
#[cfg(any(feature = "compression-gzip-stream", feature = "compression-deflate-stream"))]
fn level_to_flate2(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::Fastest => 1,
        CompressionLevel::Best => 9,
        CompressionLevel::Default => 6,
        CompressionLevel::Precise(n) => n.clamp(0, 9),
    }
}

/// Convert CompressionLevel to brotli quality (0-11).
///
/// tower-http overrides Default to 4 (NGINX default) for performance.
#[cfg(feature = "compression-br-stream")]
fn level_to_brotli(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::Fastest => 0,
        CompressionLevel::Best => 11,
        CompressionLevel::Default => 4, // tower-http's custom default
        CompressionLevel::Precise(n) => n.clamp(0, 11),
    }
}

/// Convert CompressionLevel to zstd level (1-22).
#[cfg(feature = "compression-zstd-stream")]
fn level_to_zstd(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::Fastest => 1,
        CompressionLevel::Best => 22,
        CompressionLevel::Default => 3,
        CompressionLevel::Precise(n) => (n as i32).clamp(1, 22),
    }
}

/// Compression configuration.
///
/// Used to configure compression behavior for both client and server.
#[derive(Debug, Clone, Copy)]
pub struct CompressionConfig {
    /// Minimum bytes before compression is applied.
    /// Default is 0 (compress everything), matching connect-go behavior.
    /// Messages smaller than this threshold are sent uncompressed.
    pub min_bytes: usize,
    /// Compression level/quality.
    pub level: CompressionLevel,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            min_bytes: 0,
            level: CompressionLevel::Default,
        }
    }
}

impl CompressionConfig {
    /// Create a new compression config with the specified minimum bytes threshold.
    pub fn new(min_bytes: usize) -> Self {
        Self {
            min_bytes,
            level: CompressionLevel::Default,
        }
    }

    /// Set the compression level.
    pub fn level(mut self, level: CompressionLevel) -> Self {
        self.level = level;
        self
    }

    /// Disable compression by setting threshold to usize::MAX.
    pub fn disabled() -> Self {
        Self {
            min_bytes: usize::MAX,
            level: CompressionLevel::Default,
        }
    }

    /// Check if compression is effectively disabled.
    pub fn is_disabled(&self) -> bool {
        self.min_bytes == usize::MAX
    }
}

/// Returns a comma-separated string of supported encodings for error messages.
pub fn supported_encodings_str() -> &'static str {
    // Build string based on enabled features
    // Order: gzip, deflate, br, zstd, identity
    #[cfg(all(
        feature = "compression-gzip-stream",
        feature = "compression-deflate-stream",
        feature = "compression-br-stream",
        feature = "compression-zstd-stream"
    ))]
    {
        "gzip, deflate, br, zstd, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        feature = "compression-deflate-stream",
        feature = "compression-br-stream",
        not(feature = "compression-zstd-stream")
    ))]
    {
        "gzip, deflate, br, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        feature = "compression-deflate-stream",
        not(feature = "compression-br-stream"),
        feature = "compression-zstd-stream"
    ))]
    {
        "gzip, deflate, zstd, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        feature = "compression-deflate-stream",
        not(feature = "compression-br-stream"),
        not(feature = "compression-zstd-stream")
    ))]
    {
        "gzip, deflate, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        not(feature = "compression-deflate-stream"),
        feature = "compression-br-stream",
        feature = "compression-zstd-stream"
    ))]
    {
        "gzip, br, zstd, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        not(feature = "compression-deflate-stream"),
        feature = "compression-br-stream",
        not(feature = "compression-zstd-stream")
    ))]
    {
        "gzip, br, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        not(feature = "compression-deflate-stream"),
        not(feature = "compression-br-stream"),
        feature = "compression-zstd-stream"
    ))]
    {
        "gzip, zstd, identity"
    }
    #[cfg(all(
        feature = "compression-gzip-stream",
        not(feature = "compression-deflate-stream"),
        not(feature = "compression-br-stream"),
        not(feature = "compression-zstd-stream")
    ))]
    {
        "gzip, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        feature = "compression-deflate-stream",
        feature = "compression-br-stream",
        feature = "compression-zstd-stream"
    ))]
    {
        "deflate, br, zstd, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        feature = "compression-deflate-stream",
        feature = "compression-br-stream",
        not(feature = "compression-zstd-stream")
    ))]
    {
        "deflate, br, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        feature = "compression-deflate-stream",
        not(feature = "compression-br-stream"),
        feature = "compression-zstd-stream"
    ))]
    {
        "deflate, zstd, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        feature = "compression-deflate-stream",
        not(feature = "compression-br-stream"),
        not(feature = "compression-zstd-stream")
    ))]
    {
        "deflate, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        not(feature = "compression-deflate-stream"),
        feature = "compression-br-stream",
        feature = "compression-zstd-stream"
    ))]
    {
        "br, zstd, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        not(feature = "compression-deflate-stream"),
        feature = "compression-br-stream",
        not(feature = "compression-zstd-stream")
    ))]
    {
        "br, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        not(feature = "compression-deflate-stream"),
        not(feature = "compression-br-stream"),
        feature = "compression-zstd-stream"
    ))]
    {
        "zstd, identity"
    }
    #[cfg(all(
        not(feature = "compression-gzip-stream"),
        not(feature = "compression-deflate-stream"),
        not(feature = "compression-br-stream"),
        not(feature = "compression-zstd-stream")
    ))]
    {
        "identity"
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
                let q = params.split(';').find_map(|p| p.trim().strip_prefix("q="));
                (enc.trim(), q)
            }
            None => (token, None),
        };

        // Skip if q=0 (explicitly disabled)
        if let Some(q) = q_value {
            let q = q.trim();
            if q == "0" || q == "0.0" || q == "0.00" || q == "0.000" {
                continue;
            }
        }

        // Return first supported encoding
        match encoding {
            #[cfg(feature = "compression-gzip-stream")]
            "gzip" => return CompressionEncoding::Gzip,
            #[cfg(feature = "compression-deflate-stream")]
            "deflate" => return CompressionEncoding::Deflate,
            #[cfg(feature = "compression-br-stream")]
            "br" => return CompressionEncoding::Brotli,
            #[cfg(feature = "compression-zstd-stream")]
            "zstd" => return CompressionEncoding::Zstd,
            "identity" => return CompressionEncoding::Identity,
            _ => continue,
        }
    }

    CompressionEncoding::Identity
}

/// Header name for Connect streaming request compression.
pub const CONNECT_CONTENT_ENCODING: &str = "connect-content-encoding";

/// Header name for Connect streaming response compression negotiation.
pub const CONNECT_ACCEPT_ENCODING: &str = "connect-accept-encoding";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_encoding_from_header_identity() {
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
        assert!(CompressionEncoding::Identity.codec().is_none());
        let codec = CompressionEncoding::Gzip.codec();
        assert!(codec.is_some());
        assert_eq!(codec.unwrap().name(), "gzip");
    }

    #[test]
    fn test_compression_level_precise() {
        assert_eq!(CompressionLevel::precise(5), CompressionLevel::Precise(5));
    }

    #[test]
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.min_bytes, 0);
        assert_eq!(config.level, CompressionLevel::Default);
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
        assert!(config.is_disabled());
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
        assert_eq!(
            negotiate_response_encoding(Some("gzip, identity")),
            CompressionEncoding::Gzip
        );
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_negotiate_response_encoding_q_values() {
        // q=0 means "not acceptable"
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
}
