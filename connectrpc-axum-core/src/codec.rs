//! Compression codec trait and implementations.
//!
//! This module provides the [`Codec`] trait for per-message compression
//! and implementations for common algorithms:
//! - [`GzipCodec`]: Gzip compression (requires `compression-gzip` feature)
//! - [`DeflateCodec`]: Deflate compression (requires `compression-deflate` feature)
//! - [`BrotliCodec`]: Brotli compression (requires `compression-br` feature)
//! - [`ZstdCodec`]: Zstd compression (requires `compression-zstd` feature)

use bytes::Bytes;
use std::io;
use std::sync::Arc;

#[cfg(any(
    feature = "compression-gzip-stream",
    feature = "compression-deflate-stream",
    feature = "compression-br-stream",
    feature = "compression-zstd-stream"
))]
use std::io::{Read, Write};

#[cfg(feature = "compression-gzip-stream")]
use flate2::Compression as GzipLevel;
#[cfg(feature = "compression-gzip-stream")]
use flate2::read::GzDecoder;
#[cfg(feature = "compression-gzip-stream")]
use flate2::write::GzEncoder;

/// Error returned by [`Codec::decompress_limited`].
///
/// Distinguishes a genuine decode failure ([`DecompressError::Io`]) from the
/// decompressed payload exceeding the configured size limit
/// ([`DecompressError::TooLarge`]), so callers can map the latter to a
/// `ResourceExhausted` status rather than treating it as malformed input.
#[derive(Debug)]
pub enum DecompressError {
    /// The underlying decompressor failed (corrupt/invalid input).
    Io(io::Error),
    /// The decompressed output exceeded `max_output` bytes. Reading is aborted
    /// as soon as the limit is crossed, so memory stays bounded.
    TooLarge {
        /// The limit that was exceeded, in bytes.
        limit: usize,
    },
}

impl std::fmt::Display for DecompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecompressError::Io(e) => write!(f, "{e}"),
            DecompressError::TooLarge { limit } => write!(
                f,
                "decompressed message exceeds the configured limit of {limit} bytes"
            ),
        }
    }
}

impl std::error::Error for DecompressError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecompressError::Io(e) => Some(e),
            DecompressError::TooLarge { .. } => None,
        }
    }
}

/// Decompress from `reader`, aborting once the output would exceed `max_output`.
///
/// Reads at most `max_output + 1` bytes so an overflow can be detected without
/// allocating the full (potentially enormous) decompressed payload. This is the
/// defense against decompression bombs: memory stays bounded by the limit
/// regardless of the compressed input's expansion ratio.
///
/// `max_output == usize::MAX` is treated as unbounded.
#[cfg(any(
    feature = "compression-gzip-stream",
    feature = "compression-deflate-stream",
    feature = "compression-br-stream",
    feature = "compression-zstd-stream"
))]
fn read_limited<R: Read>(reader: R, max_output: usize) -> Result<Bytes, DecompressError> {
    // +1 so a payload of exactly `max_output` bytes succeeds while anything
    // larger is detected. Saturating so usize::MAX (unbounded) stays unbounded.
    let cap = (max_output as u64).saturating_add(1);
    let mut out = Vec::new();
    reader
        .take(cap)
        .read_to_end(&mut out)
        .map_err(DecompressError::Io)?;
    if out.len() > max_output {
        return Err(DecompressError::TooLarge { limit: max_output });
    }
    Ok(Bytes::from(out))
}

/// Codec trait for per-message (envelope) compression.
///
/// Used for streaming Connect RPCs where each message is individually compressed.
/// HTTP body compression for unary RPCs is typically handled by middleware.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_core::Codec;
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

    /// Decompress data, aborting if the output would exceed `max_output` bytes.
    ///
    /// This is the bomb-safe entry point: callers pass their receive limit so a
    /// small compressed payload cannot expand without bound. Built-in codecs
    /// override this to cap memory *while reading* (see [`read_limited`]).
    ///
    /// The default implementation decompresses fully via [`Codec::decompress`]
    /// and then checks the size — it enforces the limit but does **not** bound
    /// peak memory. Custom codecs handling untrusted input should override this
    /// with a streaming, output-bounded implementation.
    ///
    /// `max_output == usize::MAX` is treated as unbounded.
    fn decompress_limited(&self, data: &[u8], max_output: usize) -> Result<Bytes, DecompressError> {
        let out = self.decompress(data).map_err(DecompressError::Io)?;
        if out.len() > max_output {
            return Err(DecompressError::TooLarge { limit: max_output });
        }
        Ok(out)
    }
}

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

    /// Decompress data, aborting if the output would exceed `max_output` bytes.
    ///
    /// See [`Codec::decompress_limited`]. Use this when decompressing untrusted
    /// input to guard against decompression bombs.
    pub fn decompress_limited(
        &self,
        data: &[u8],
        max_output: usize,
    ) -> Result<Bytes, DecompressError> {
        self.0.decompress_limited(data, max_output)
    }
}

impl std::fmt::Debug for BoxedCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BoxedCodec").field(&self.name()).finish()
    }
}

/// Gzip codec using flate2.
///
/// Requires the `compression-gzip` feature.
#[cfg(feature = "compression-gzip-stream")]
#[derive(Debug, Clone, Copy)]
pub struct GzipCodec {
    /// Compression level (0-9). Default is 6.
    pub level: u32,
}

#[cfg(feature = "compression-gzip-stream")]
impl Default for GzipCodec {
    fn default() -> Self {
        Self { level: 6 }
    }
}

#[cfg(feature = "compression-gzip-stream")]
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

#[cfg(feature = "compression-gzip-stream")]
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

    fn decompress_limited(&self, data: &[u8], max_output: usize) -> Result<Bytes, DecompressError> {
        read_limited(GzDecoder::new(data), max_output)
    }
}

/// Identity codec (no compression).
///
/// This codec passes data through unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityCodec;

impl Codec for IdentityCodec {
    fn name(&self) -> &'static str {
        "identity"
    }

    fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        Ok(Bytes::copy_from_slice(data))
    }

    fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
        Ok(Bytes::copy_from_slice(data))
    }
}

/// Deflate codec using flate2 (zlib format).
///
/// Note: HTTP "deflate" Content-Encoding uses zlib format (RFC 1950),
/// not raw DEFLATE (RFC 1951).
///
/// Requires the `compression-deflate` feature.
#[cfg(feature = "compression-deflate-stream")]
#[derive(Debug, Clone, Copy)]
pub struct DeflateCodec {
    /// Compression level (0-9). Default is 6.
    pub level: u32,
}

#[cfg(feature = "compression-deflate-stream")]
impl Default for DeflateCodec {
    fn default() -> Self {
        Self { level: 6 }
    }
}

#[cfg(feature = "compression-deflate-stream")]
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

#[cfg(feature = "compression-deflate-stream")]
impl Codec for DeflateCodec {
    fn name(&self) -> &'static str {
        "deflate"
    }

    fn compress(&self, data: &[u8]) -> io::Result<Bytes> {
        use flate2::write::ZlibEncoder;
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::new(self.level));
        encoder.write_all(data)?;
        Ok(Bytes::from(encoder.finish()?))
    }

    fn decompress(&self, data: &[u8]) -> io::Result<Bytes> {
        use flate2::read::ZlibDecoder;
        let mut decoder = ZlibDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(Bytes::from(decompressed))
    }

    fn decompress_limited(&self, data: &[u8], max_output: usize) -> Result<Bytes, DecompressError> {
        read_limited(flate2::read::ZlibDecoder::new(data), max_output)
    }
}

/// Brotli codec.
///
/// Requires the `compression-br` feature.
#[cfg(feature = "compression-br-stream")]
#[derive(Debug, Clone, Copy)]
pub struct BrotliCodec {
    /// Compression quality (0-11). Default is 4.
    pub quality: u32,
}

#[cfg(feature = "compression-br-stream")]
impl Default for BrotliCodec {
    fn default() -> Self {
        Self { quality: 4 }
    }
}

#[cfg(feature = "compression-br-stream")]
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

#[cfg(feature = "compression-br-stream")]
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

    fn decompress_limited(&self, data: &[u8], max_output: usize) -> Result<Bytes, DecompressError> {
        // `Decompressor` is the streaming `Read` adapter, letting `read_limited`
        // stop before a bomb fully expands. 4096 is the internal read buffer.
        let reader = brotli::Decompressor::new(std::io::Cursor::new(data), 4096);
        read_limited(reader, max_output)
    }
}

/// Zstd codec.
///
/// Requires the `compression-zstd` feature.
#[cfg(feature = "compression-zstd-stream")]
#[derive(Debug, Clone, Copy)]
pub struct ZstdCodec {
    /// Compression level (1-22). Default is 3.
    pub level: i32,
}

#[cfg(feature = "compression-zstd-stream")]
impl Default for ZstdCodec {
    fn default() -> Self {
        Self { level: 3 }
    }
}

#[cfg(feature = "compression-zstd-stream")]
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

#[cfg(feature = "compression-zstd-stream")]
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

    fn decompress_limited(&self, data: &[u8], max_output: usize) -> Result<Bytes, DecompressError> {
        let decoder = zstd::Decoder::new(data).map_err(DecompressError::Io)?;
        read_limited(decoder, max_output)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(feature = "compression-gzip-stream")]
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
    fn test_identity_codec() {
        let codec = IdentityCodec;
        assert_eq!(codec.name(), "identity");

        let original = b"Hello, World!";
        let compressed = codec.compress(original).unwrap();
        assert_eq!(&compressed[..], &original[..]);

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[cfg(feature = "compression-gzip-stream")]
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

    #[cfg(feature = "compression-gzip-stream")]
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

        let compressed = compress_bytes(original.clone(), None).unwrap();
        assert_eq!(compressed, original);

        let decompressed = decompress_bytes(compressed, None).unwrap();
        assert_eq!(decompressed, original);
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_decompress_invalid_gzip() {
        let codec = BoxedCodec::new(GzipCodec::default());
        let invalid = b"not valid gzip data";
        let result = codec.decompress(invalid);
        assert!(result.is_err());
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_decompress_limited_rejects_bomb() {
        let codec = GzipCodec::default();
        // 1 MiB of zeros compresses to ~1 KiB but expands far past the limit.
        let bomb = codec.compress(&vec![0u8; 1024 * 1024]).unwrap();
        assert!(bomb.len() < 64 * 1024, "compressed bomb should be small");

        // Decompressing under a small limit must abort with TooLarge.
        match codec.decompress_limited(&bomb, 64 * 1024) {
            Err(DecompressError::TooLarge { limit }) => assert_eq!(limit, 64 * 1024),
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_decompress_limited_allows_within_limit() {
        let codec = GzipCodec::default();
        let original = vec![7u8; 4096];
        let compressed = codec.compress(&original).unwrap();

        // Exactly at the limit succeeds.
        let out = codec
            .decompress_limited(&compressed, original.len())
            .unwrap();
        assert_eq!(&out[..], &original[..]);

        // One byte under the limit fails.
        match codec.decompress_limited(&compressed, original.len() - 1) {
            Err(DecompressError::TooLarge { .. }) => {}
            other => panic!("expected TooLarge, got {other:?}"),
        }

        // Unlimited (usize::MAX) always succeeds.
        let out = codec.decompress_limited(&compressed, usize::MAX).unwrap();
        assert_eq!(&out[..], &original[..]);
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_decompress_limited_propagates_io_error() {
        let codec = GzipCodec::default();
        match codec.decompress_limited(b"not valid gzip data", 1024) {
            Err(DecompressError::Io(_)) => {}
            other => panic!("expected Io error, got {other:?}"),
        }
    }

    // Per-codec bomb coverage: each built-in codec uses a distinct reader adapter
    // inside `decompress_limited`, so prove the bound holds for each one with a
    // payload that compresses small but decompresses past the limit.
    #[cfg(feature = "compression-deflate-stream")]
    #[test]
    fn test_deflate_decompress_limited_rejects_bomb() {
        let codec = DeflateCodec::default();
        let bomb = codec.compress(&vec![0u8; 1024 * 1024]).unwrap();
        assert!(bomb.len() < 64 * 1024, "compressed bomb should be small");
        match codec.decompress_limited(&bomb, 64 * 1024) {
            Err(DecompressError::TooLarge { limit }) => assert_eq!(limit, 64 * 1024),
            other => panic!("expected TooLarge, got {other:?}"),
        }
        // And a within-limit payload still round-trips.
        let small = codec.compress(&vec![3u8; 4096]).unwrap();
        let out = codec.decompress_limited(&small, 64 * 1024).unwrap();
        assert_eq!(out.len(), 4096);
    }

    #[cfg(feature = "compression-br-stream")]
    #[test]
    fn test_brotli_decompress_limited_rejects_bomb() {
        let codec = BrotliCodec::default();
        let bomb = codec.compress(&vec![0u8; 1024 * 1024]).unwrap();
        assert!(bomb.len() < 64 * 1024, "compressed bomb should be small");
        match codec.decompress_limited(&bomb, 64 * 1024) {
            Err(DecompressError::TooLarge { limit }) => assert_eq!(limit, 64 * 1024),
            other => panic!("expected TooLarge, got {other:?}"),
        }
        let small = codec.compress(&vec![3u8; 4096]).unwrap();
        let out = codec.decompress_limited(&small, 64 * 1024).unwrap();
        assert_eq!(out.len(), 4096);
    }

    #[cfg(feature = "compression-zstd-stream")]
    #[test]
    fn test_zstd_decompress_limited_rejects_bomb() {
        let codec = ZstdCodec::default();
        let bomb = codec.compress(&vec![0u8; 1024 * 1024]).unwrap();
        assert!(bomb.len() < 64 * 1024, "compressed bomb should be small");
        match codec.decompress_limited(&bomb, 64 * 1024) {
            Err(DecompressError::TooLarge { limit }) => assert_eq!(limit, 64 * 1024),
            other => panic!("expected TooLarge, got {other:?}"),
        }
        let small = codec.compress(&vec![3u8; 4096]).unwrap();
        let out = codec.decompress_limited(&small, 64 * 1024).unwrap();
        assert_eq!(out.len(), 4096);
    }

    #[test]
    fn test_decompress_limited_default_enforces_limit() {
        // IdentityCodec uses the default `decompress_limited` (decompress + check).
        let codec = IdentityCodec;
        assert!(codec.decompress_limited(b"hello", 10).is_ok());
        match codec.decompress_limited(b"hello world", 5) {
            Err(DecompressError::TooLarge { limit }) => assert_eq!(limit, 5),
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_boxed_codec_debug() {
        let codec = BoxedCodec::new(GzipCodec::default());
        let debug_str = format!("{:?}", codec);
        assert!(debug_str.contains("BoxedCodec"));
        assert!(debug_str.contains("gzip"));
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

    #[cfg(feature = "compression-br-stream")]
    #[test]
    fn test_brotli_codec_with_quality() {
        let codec = BrotliCodec::with_quality(11);
        assert_eq!(codec.quality, 11);

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
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

    #[cfg(feature = "compression-zstd-stream")]
    #[test]
    fn test_zstd_codec_with_level() {
        let codec = ZstdCodec::with_level(19);
        assert_eq!(codec.level, 19);

        let original = b"Hello, World! This is a test message.";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }
}
