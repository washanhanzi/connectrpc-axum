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
///     fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
///         // ... lz4 compression
///     }
///
///     fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
///         // ... lz4 decompression
///     }
/// }
/// ```
pub trait Codec: Send + Sync + 'static {
    /// The encoding name for HTTP headers (e.g., "gzip", "zstd", "br").
    fn name(&self) -> &'static str;

    /// Compress data.
    fn compress(&self, data: &[u8]) -> io::Result<Bytes>;

    /// Decompress data.
    fn decompress(&self, data: &[u8]) -> io::Result<Bytes>;
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
    pub fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        self.0.compress(data)
    }

    /// Decompress data.
    pub fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
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

    fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        let mut encoder = GzEncoder::new(Vec::new(), GzipLevel::new(self.level));
        encoder.write_all(data)?;
        Ok(Bytes::from(encoder.finish()?))
    }

    fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(Bytes::from(decompressed))
    }
}

// ============================================================================
// Deflate Codec (feature-gated)
// ============================================================================

/// Deflate codec using flate2.
///
/// Requires the `compression-deflate` feature.
#[cfg(feature = "compression-deflate")]
#[derive(Debug, Clone, Copy)]
pub struct DeflateCodec {
    /// Compression level (0-9). Default is 6.
    pub level: u32,
}

#[cfg(feature = "compression-deflate")]
impl Default for DeflateCodec {
    fn default() -> Self {
        Self { level: 6 }
    }
}

#[cfg(feature = "compression-deflate")]
impl DeflateCodec {
    /// Create a new DeflateCodec with the specified compression level.
    ///
    /// Level ranges from 0 (no compression) to 9 (best compression).
    pub fn with_level(level: u32) -> Self {
        Self {
            level: level.min(9),
        }
    }
}

#[cfg(feature = "compression-deflate")]
impl Codec for DeflateCodec {
    fn name(&self) -> &'static str {
        "deflate"
    }

    fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        // HTTP "deflate" Content-Encoding uses zlib format (RFC 1950),
        // not raw DEFLATE (RFC 1951). This ensures compatibility with
        // tower-http and standard HTTP clients.
        use flate2::write::ZlibEncoder;
        let mut encoder = ZlibEncoder::new(Vec::new(), GzipLevel::new(self.level));
        encoder.write_all(data)?;
        Ok(Bytes::from(encoder.finish()?))
    }

    fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
        // HTTP "deflate" Content-Encoding uses zlib format (RFC 1950),
        // not raw DEFLATE (RFC 1951). This ensures compatibility with
        // tower-http and standard HTTP clients.
        use flate2::read::ZlibDecoder;
        let mut decoder = ZlibDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(Bytes::from(decompressed))
    }
}

// ============================================================================
// Brotli Codec (feature-gated)
// ============================================================================

/// Brotli codec.
///
/// Requires the `compression-br` feature.
#[cfg(feature = "compression-br")]
#[derive(Debug, Clone, Copy)]
pub struct BrotliCodec {
    /// Compression quality (0-11). Default is 4.
    pub quality: u32,
}

#[cfg(feature = "compression-br")]
impl Default for BrotliCodec {
    fn default() -> Self {
        Self { quality: 4 }
    }
}

#[cfg(feature = "compression-br")]
impl BrotliCodec {
    /// Create a new BrotliCodec with the specified quality level.
    ///
    /// Quality ranges from 0 (fastest) to 11 (best compression).
    pub fn with_quality(quality: u32) -> Self {
        Self {
            quality: quality.min(11),
        }
    }
}

#[cfg(feature = "compression-br")]
impl Codec for BrotliCodec {
    fn name(&self) -> &'static str {
        "br"
    }

    fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        use brotli::enc::BrotliEncoderParams;
        let mut output = Vec::new();
        let params = BrotliEncoderParams {
            quality: self.quality as i32,
            ..Default::default()
        };
        brotli::enc::BrotliCompress(&mut std::io::Cursor::new(data), &mut output, &params)?;
        Ok(Bytes::from(output))
    }

    fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
        let mut output = Vec::new();
        brotli::BrotliDecompress(&mut std::io::Cursor::new(data), &mut output)?;
        Ok(Bytes::from(output))
    }
}

// ============================================================================
// Zstd Codec (feature-gated)
// ============================================================================

/// Zstd codec.
///
/// Requires the `compression-zstd` feature.
#[cfg(feature = "compression-zstd")]
#[derive(Debug, Clone, Copy)]
pub struct ZstdCodec {
    /// Compression level (1-22). Default is 3.
    pub level: i32,
}

#[cfg(feature = "compression-zstd")]
impl Default for ZstdCodec {
    fn default() -> Self {
        Self { level: 3 }
    }
}

#[cfg(feature = "compression-zstd")]
impl ZstdCodec {
    /// Create a new ZstdCodec with the specified compression level.
    ///
    /// Level ranges from 1 (fastest) to 22 (best compression).
    pub fn with_level(level: i32) -> Self {
        Self {
            level: level.clamp(1, 22),
        }
    }
}

#[cfg(feature = "compression-zstd")]
impl Codec for ZstdCodec {
    fn name(&self) -> &'static str {
        "zstd"
    }

    fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        let compressed = zstd::bulk::compress(data, self.level)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(Bytes::from(compressed))
    }

    fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
        let mut decoder = zstd::Decoder::new(data)?;
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(Bytes::from(decompressed))
    }
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
        #[cfg(feature = "compression-deflate")]
        "deflate" => Ok(Some(BoxedCodec::new(DeflateCodec::default()))),
        #[cfg(feature = "compression-br")]
        "br" => Ok(Some(BoxedCodec::new(BrotliCodec::default()))),
        #[cfg(feature = "compression-zstd")]
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

/// Returns a comma-separated string of supported encodings for error messages.
fn supported_encodings_str() -> &'static str {
    #[cfg(all(
        feature = "compression-deflate",
        feature = "compression-br",
        feature = "compression-zstd"
    ))]
    {
        "gzip, deflate, br, zstd, identity"
    }
    #[cfg(all(
        feature = "compression-deflate",
        feature = "compression-br",
        not(feature = "compression-zstd")
    ))]
    {
        "gzip, deflate, br, identity"
    }
    #[cfg(all(
        feature = "compression-deflate",
        not(feature = "compression-br"),
        feature = "compression-zstd"
    ))]
    {
        "gzip, deflate, zstd, identity"
    }
    #[cfg(all(
        not(feature = "compression-deflate"),
        feature = "compression-br",
        feature = "compression-zstd"
    ))]
    {
        "gzip, br, zstd, identity"
    }
    #[cfg(all(
        feature = "compression-deflate",
        not(feature = "compression-br"),
        not(feature = "compression-zstd")
    ))]
    {
        "gzip, deflate, identity"
    }
    #[cfg(all(
        not(feature = "compression-deflate"),
        feature = "compression-br",
        not(feature = "compression-zstd")
    ))]
    {
        "gzip, br, identity"
    }
    #[cfg(all(
        not(feature = "compression-deflate"),
        not(feature = "compression-br"),
        feature = "compression-zstd"
    ))]
    {
        "gzip, zstd, identity"
    }
    #[cfg(all(
        not(feature = "compression-deflate"),
        not(feature = "compression-br"),
        not(feature = "compression-zstd")
    ))]
    {
        "gzip, identity"
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
        Some(c) => c.compress(&bytes),
    }
}

/// Decompress bytes using the specified codec.
///
/// If `codec` is `None`, returns the input unchanged (identity).
pub fn decompress_bytes(bytes: Bytes, codec: Option<&BoxedCodec>) -> io::Result<Bytes> {
    match codec {
        None => Ok(bytes), // identity: zero-copy passthrough
        Some(c) => c.decompress(&bytes),
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
    #[cfg(feature = "compression-deflate")]
    Deflate,
    #[cfg(feature = "compression-br")]
    Brotli,
    #[cfg(feature = "compression-zstd")]
    Zstd,
}

impl CompressionEncoding {
    /// Parse from Content-Encoding or Connect-Content-Encoding header value.
    /// Returns None for unsupported encodings (caller should return Unimplemented).
    pub fn from_header(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("identity") | Some("") => Some(Self::Identity),
            Some("gzip") => Some(Self::Gzip),
            #[cfg(feature = "compression-deflate")]
            Some("deflate") => Some(Self::Deflate),
            #[cfg(feature = "compression-br")]
            Some("br") => Some(Self::Brotli),
            #[cfg(feature = "compression-zstd")]
            Some("zstd") => Some(Self::Zstd),
            _ => None, // unsupported
        }
    }

    /// Get the header value string for this encoding.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
            #[cfg(feature = "compression-deflate")]
            Self::Deflate => "deflate",
            #[cfg(feature = "compression-br")]
            Self::Brotli => "br",
            #[cfg(feature = "compression-zstd")]
            Self::Zstd => "zstd",
        }
    }

    /// Get the codec for this encoding.
    ///
    /// Returns `None` for identity, `Some(BoxedCodec)` for others.
    pub fn codec(&self) -> Option<BoxedCodec> {
        match self {
            Self::Identity => None,
            Self::Gzip => Some(BoxedCodec::new(GzipCodec::default())),
            #[cfg(feature = "compression-deflate")]
            Self::Deflate => Some(BoxedCodec::new(DeflateCodec::default())),
            #[cfg(feature = "compression-br")]
            Self::Brotli => Some(BoxedCodec::new(BrotliCodec::default())),
            #[cfg(feature = "compression-zstd")]
            Self::Zstd => Some(BoxedCodec::new(ZstdCodec::default())),
        }
    }

    /// Get the codec for this encoding with the specified compression level.
    ///
    /// Returns `None` for identity, `Some(BoxedCodec)` for others.
    /// The level is converted to algorithm-specific values matching tower-http behavior.
    pub fn codec_with_level(&self, level: CompressionLevel) -> Option<BoxedCodec> {
        match self {
            Self::Identity => None,
            Self::Gzip => Some(BoxedCodec::new(GzipCodec::with_level(level_to_flate2(level)))),
            #[cfg(feature = "compression-deflate")]
            Self::Deflate => Some(BoxedCodec::new(DeflateCodec::with_level(level_to_flate2(
                level,
            )))),
            #[cfg(feature = "compression-br")]
            Self::Brotli => Some(BoxedCodec::new(BrotliCodec::with_quality(level_to_brotli(
                level,
            )))),
            #[cfg(feature = "compression-zstd")]
            Self::Zstd => Some(BoxedCodec::new(ZstdCodec::with_level(level_to_zstd(level)))),
        }
    }
}

// ============================================================================
// Compression Level Conversion
// ============================================================================

/// Convert CompressionLevel to flate2 gzip/deflate level (0-9).
///
/// Matches tower-http → async_compression → compression_codecs behavior:
/// - `Fastest` → 1 (flate2::Compression::fast())
/// - `Best` → 9 (flate2::Compression::best())
/// - `Default` → 6 (flate2::Compression::default())
/// - `Precise(n)` → n clamped to 0-9
fn level_to_flate2(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::Fastest => 1,
        CompressionLevel::Best => 9,
        CompressionLevel::Default => 6,
        CompressionLevel::Precise(n) => n.clamp(0, 9) as u32,
        _ => 6, // Future variants: use default
    }
}

/// Convert CompressionLevel to brotli quality (0-11).
///
/// tower-http overrides Default to 4 (NGINX default) for performance.
/// The brotli library default is 11, which is too slow for on-the-fly compression.
#[cfg(feature = "compression-br")]
fn level_to_brotli(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::Fastest => 0,
        CompressionLevel::Best => 11,
        CompressionLevel::Default => 4, // tower-http's custom default (not 11)
        CompressionLevel::Precise(n) => n.clamp(0, 11) as u32,
        _ => 4, // Future variants: use default
    }
}

/// Convert CompressionLevel to zstd level (1-22).
///
/// Note: zstd supports negative levels but we follow tower-http/async-compression
/// which uses 1 as "fastest" (negative levels produce larger outputs).
#[cfg(feature = "compression-zstd")]
fn level_to_zstd(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::Fastest => 1,   // OUR_FASTEST in async-compression
        CompressionLevel::Best => 22,     // libzstd max
        CompressionLevel::Default => 3,   // libzstd::DEFAULT_COMPRESSION_LEVEL
        CompressionLevel::Precise(n) => (n as i32).clamp(1, 22),
        _ => 3, // Future variants: use default
    }
}

// ============================================================================
// Compression Configuration
// ============================================================================

/// Re-export tower-http's CompressionLevel for unified compression configuration.
///
/// This is used for both:
/// - Tower's HTTP body compression (unary RPCs via `CompressionLayer`)
/// - Envelope compression (streaming RPCs via `GzipCodec`, etc.)
pub use tower_http::CompressionLevel;

/// Server compression configuration.
#[derive(Debug, Clone, Copy)]
pub struct CompressionConfig {
    /// Minimum bytes before compression is applied.
    /// Default is 0 (compress everything), matching connect-go behavior.
    /// Messages smaller than this threshold are sent uncompressed.
    pub min_bytes: usize,
    /// Compression level/quality.
    /// Default is `CompressionLevel::Default` which varies by algorithm:
    /// - gzip: level 4
    /// - brotli: level 4
    /// - zstd: level 3
    /// - deflate: level 4
    pub level: CompressionLevel,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        // Connect-go default is 0 (compress everything)
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
    ///
    /// # Examples
    ///
    /// ```rust
    /// use connectrpc_axum::context::CompressionConfig;
    /// use connectrpc_axum::context::CompressionLevel;
    ///
    /// // Use fastest compression
    /// let config = CompressionConfig::default().level(CompressionLevel::Fastest);
    ///
    /// // Use best compression
    /// let config = CompressionConfig::default().level(CompressionLevel::Best);
    ///
    /// // Use precise level (algorithm-specific)
    /// let config = CompressionConfig::default().level(CompressionLevel::Precise(6));
    /// ```
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
            #[cfg(feature = "compression-deflate")]
            "deflate" => return CompressionEncoding::Deflate,
            #[cfg(feature = "compression-br")]
            "br" => return CompressionEncoding::Brotli,
            #[cfg(feature = "compression-zstd")]
            "zstd" => return CompressionEncoding::Zstd,
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

        // Feature-gated: supported when feature enabled, None otherwise
        #[cfg(feature = "compression-br")]
        assert_eq!(
            CompressionEncoding::from_header(Some("br")),
            Some(CompressionEncoding::Brotli)
        );
        #[cfg(not(feature = "compression-br"))]
        assert_eq!(CompressionEncoding::from_header(Some("br")), None);

        #[cfg(feature = "compression-deflate")]
        assert_eq!(
            CompressionEncoding::from_header(Some("deflate")),
            Some(CompressionEncoding::Deflate)
        );
        #[cfg(not(feature = "compression-deflate"))]
        assert_eq!(CompressionEncoding::from_header(Some("deflate")), None);

        #[cfg(feature = "compression-zstd")]
        assert_eq!(
            CompressionEncoding::from_header(Some("zstd")),
            Some(CompressionEncoding::Zstd)
        );
        #[cfg(not(feature = "compression-zstd"))]
        assert_eq!(CompressionEncoding::from_header(Some("zstd")), None);

        // Always unsupported
        assert_eq!(CompressionEncoding::from_header(Some("lz4")), None);
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

        // deflate first - depends on feature
        #[cfg(feature = "compression-deflate")]
        assert_eq!(
            negotiate_response_encoding(Some("deflate, gzip")),
            CompressionEncoding::Deflate
        );
        #[cfg(not(feature = "compression-deflate"))]
        assert_eq!(
            negotiate_response_encoding(Some("deflate, gzip")),
            CompressionEncoding::Gzip
        );

        // No gzip, only unsupported algorithms (when features disabled)
        #[cfg(all(not(feature = "compression-deflate"), not(feature = "compression-br")))]
        assert_eq!(
            negotiate_response_encoding(Some("deflate, br")),
            CompressionEncoding::Identity
        );
        // When deflate feature is enabled
        #[cfg(all(feature = "compression-deflate", not(feature = "compression-br")))]
        assert_eq!(
            negotiate_response_encoding(Some("deflate, br")),
            CompressionEncoding::Deflate
        );
        // When br feature is enabled (but not deflate)
        #[cfg(all(not(feature = "compression-deflate"), feature = "compression-br"))]
        assert_eq!(
            negotiate_response_encoding(Some("deflate, br")),
            CompressionEncoding::Brotli
        );
        // When both features are enabled
        #[cfg(all(feature = "compression-deflate", feature = "compression-br"))]
        assert_eq!(
            negotiate_response_encoding(Some("deflate, br")),
            CompressionEncoding::Deflate
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
        #[cfg(feature = "compression-br")]
        assert_eq!(
            negotiate_response_encoding(Some("br, gzip")),
            CompressionEncoding::Brotli
        );
        #[cfg(not(feature = "compression-br"))]
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

        #[cfg(feature = "compression-br")]
        assert_eq!(
            negotiate_response_encoding(Some("br, zstd, gzip")),
            CompressionEncoding::Brotli
        );
        #[cfg(all(not(feature = "compression-br"), feature = "compression-zstd"))]
        assert_eq!(
            negotiate_response_encoding(Some("br, zstd, gzip")),
            CompressionEncoding::Zstd
        );
        #[cfg(all(not(feature = "compression-br"), not(feature = "compression-zstd")))]
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
        // When br feature is enabled, br;q=1 is accepted
        #[cfg(feature = "compression-br")]
        assert_eq!(
            negotiate_response_encoding(Some("br;q=1, gzip;q=0, identity")),
            CompressionEncoding::Brotli
        );
        // When br feature is disabled, br is skipped, gzip;q=0 is skipped, identity is returned
        #[cfg(not(feature = "compression-br"))]
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

        // br first with whitespace
        #[cfg(feature = "compression-br")]
        assert_eq!(
            negotiate_response_encoding(Some("br ,  gzip")),
            CompressionEncoding::Brotli
        );
        #[cfg(not(feature = "compression-br"))]
        assert_eq!(
            negotiate_response_encoding(Some("br ,  gzip")),
            CompressionEncoding::Gzip
        );
    }

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
    fn test_gzip_codec_with_level() {
        let codec = GzipCodec::with_level(9);
        assert_eq!(codec.level, 9);

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[test]
    fn test_boxed_codec() {
        let codec = BoxedCodec::new(GzipCodec::default());
        assert_eq!(codec.name(), "gzip");

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        assert_ne!(&compressed[..], &original[..]);

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
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

        // Deflate - feature gated
        #[cfg(feature = "compression-deflate")]
        {
            let codec = resolve_codec("deflate").unwrap();
            assert!(codec.is_some());
            assert_eq!(codec.unwrap().name(), "deflate");
        }
        #[cfg(not(feature = "compression-deflate"))]
        assert!(resolve_codec("deflate").is_err());

        // Brotli - feature gated
        #[cfg(feature = "compression-br")]
        {
            let codec = resolve_codec("br").unwrap();
            assert!(codec.is_some());
            assert_eq!(codec.unwrap().name(), "br");
        }
        #[cfg(not(feature = "compression-br"))]
        assert!(resolve_codec("br").is_err());

        // Zstd - feature gated
        #[cfg(feature = "compression-zstd")]
        {
            let codec = resolve_codec("zstd").unwrap();
            assert!(codec.is_some());
            assert_eq!(codec.unwrap().name(), "zstd");
        }
        #[cfg(not(feature = "compression-zstd"))]
        assert!(resolve_codec("zstd").is_err());

        // Always unsupported
        assert!(resolve_codec("lz4").is_err());
    }

    #[test]
    fn test_decompress_invalid_gzip() {
        let codec = BoxedCodec::new(GzipCodec::default());
        let invalid = b"not valid gzip data";
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

    // Feature-gated codec tests
    #[cfg(feature = "compression-deflate")]
    #[test]
    fn test_deflate_codec_compress_decompress() {
        let codec = DeflateCodec::default();
        assert_eq!(codec.name(), "deflate");

        let original = b"Hello, World! This is a test message for deflate.";
        let compressed = codec.compress(original).unwrap();
        assert_ne!(&compressed[..], &original[..]);

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[cfg(feature = "compression-deflate")]
    #[test]
    fn test_deflate_codec_with_level() {
        let codec = DeflateCodec::with_level(9);
        assert_eq!(codec.level, 9);

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[cfg(feature = "compression-br")]
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

    #[cfg(feature = "compression-br")]
    #[test]
    fn test_brotli_codec_with_quality() {
        let codec = BrotliCodec::with_quality(11);
        assert_eq!(codec.quality, 11);

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[cfg(feature = "compression-zstd")]
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

    #[cfg(feature = "compression-zstd")]
    #[test]
    fn test_zstd_codec_with_level() {
        let codec = ZstdCodec::with_level(19);
        assert_eq!(codec.level, 19);

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[test]
    fn test_codec_with_level_produces_different_compression() {
        // Use sufficiently large data to see compression differences
        let original: Vec<u8> = (0..10000)
            .map(|i| ((i % 256) as u8).wrapping_add((i / 256) as u8))
            .collect();

        // Test gzip with different levels
        let codec_fastest = CompressionEncoding::Gzip
            .codec_with_level(CompressionLevel::Fastest)
            .unwrap();
        let codec_best = CompressionEncoding::Gzip
            .codec_with_level(CompressionLevel::Best)
            .unwrap();

        let compressed_fastest = codec_fastest.compress(&original).unwrap();
        let compressed_best = codec_best.compress(&original).unwrap();

        // Best compression should produce smaller output (or equal in edge cases)
        assert!(
            compressed_best.len() <= compressed_fastest.len(),
            "Best compression ({}) should be <= fastest ({})",
            compressed_best.len(),
            compressed_fastest.len()
        );

        // Both should decompress correctly
        let decompressed_fastest = codec_fastest.decompress(&compressed_fastest).unwrap();
        let decompressed_best = codec_best.decompress(&compressed_best).unwrap();
        assert_eq!(decompressed_fastest, original);
        assert_eq!(decompressed_best, original);
    }

    #[test]
    fn test_codec_with_level_vs_default_codec() {
        // Verify that codec_with_level(Default) produces same results as codec()
        let original = b"Hello, World! This is a test message for compression level comparison.";

        let codec_default = CompressionEncoding::Gzip.codec().unwrap();
        let codec_with_default = CompressionEncoding::Gzip
            .codec_with_level(CompressionLevel::Default)
            .unwrap();

        let compressed_default = codec_default.compress(original).unwrap();
        let compressed_with_default = codec_with_default.compress(original).unwrap();

        // Should produce identical output since both use default level
        assert_eq!(compressed_default, compressed_with_default);
    }

    #[test]
    fn test_codec_with_level_identity_returns_none() {
        // Identity encoding should return None for any level
        assert!(CompressionEncoding::Identity
            .codec_with_level(CompressionLevel::Default)
            .is_none());
        assert!(CompressionEncoding::Identity
            .codec_with_level(CompressionLevel::Best)
            .is_none());
        assert!(CompressionEncoding::Identity
            .codec_with_level(CompressionLevel::Fastest)
            .is_none());
    }
}
