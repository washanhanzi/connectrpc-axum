//! Connect streaming envelope framing.
//!
//! The Connect protocol uses envelope framing for streaming RPCs:
//!
//! ```text
//! [flags:1][length:4][payload:length]
//! ```
//!
//! This module provides constants and functions for working with envelopes.

use bytes::Bytes;

use crate::codec::{BoxedCodec, DecompressError};
use crate::compression::CompressionEncoding;
use crate::error::EnvelopeError;

/// Connect streaming envelope flags.
pub mod envelope_flags {
    /// Regular message (uncompressed).
    pub const MESSAGE: u8 = 0x00;
    /// Compressed message.
    pub const COMPRESSED: u8 = 0x01;
    /// End of stream.
    pub const END_STREAM: u8 = 0x02;
}

/// Envelope header size (flags + length).
pub const ENVELOPE_HEADER_SIZE: usize = 5;

/// Wrap payload in a Connect streaming frame envelope.
///
/// Frame format: `[flags:1][length:4][payload]`
///
/// # Arguments
/// - `payload`: The message bytes to wrap
/// - `compressed`: Whether the payload is compressed (sets flag 0x01)
///
/// # Errors
/// Returns [`EnvelopeError::PayloadTooLarge`] if the payload length does not
/// fit in the envelope's 4-byte length prefix.
pub fn wrap_envelope(payload: &[u8], compressed: bool) -> Result<Vec<u8>, EnvelopeError> {
    let Ok(length) = u32::try_from(payload.len()) else {
        return Err(EnvelopeError::PayloadTooLarge {
            size: payload.len(),
        });
    };

    let flags = if compressed {
        envelope_flags::COMPRESSED
    } else {
        envelope_flags::MESSAGE
    };

    let mut frame = Vec::with_capacity(ENVELOPE_HEADER_SIZE + payload.len());
    frame.push(flags);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Parse envelope header from bytes.
///
/// Returns `(flags, length)` if successful.
///
/// # Errors
/// Returns an error if there aren't enough bytes for the header.
pub fn parse_envelope_header(data: &[u8]) -> Result<(u8, u32), EnvelopeError> {
    if data.len() < ENVELOPE_HEADER_SIZE {
        return Err(EnvelopeError::IncompleteHeader {
            expected: ENVELOPE_HEADER_SIZE,
            actual: data.len(),
        });
    }

    let flags = data[0];
    let length = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);

    Ok((flags, length))
}

/// Processed envelope payload, classified by frame type.
///
/// Returned by [`process_envelope_payload`]. Both variants carry the
/// decompressed payload bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvelopePayload {
    /// A message frame payload (END_STREAM bit clear).
    Message(Bytes),
    /// An end-stream frame payload (END_STREAM bit set, e.g. 0x02 or 0x03).
    EndStream(Bytes),
}

/// Process envelope payload based on flags, with optional decompression.
///
/// Given the flags byte and payload bytes from an envelope, validates the flags
/// and decompresses the payload if needed.
///
/// Flags are a bitfield per the Connect spec, so they are matched with bitwise
/// masks rather than exact equality: a compressed end-stream frame is `0x03`
/// (`COMPRESSED | END_STREAM`), not a distinct value.
///
/// `max_output` caps the (decompressed) payload size to defend against
/// decompression bombs: decoding aborts as soon as the output would exceed it,
/// so peak memory stays bounded regardless of the compressed input's expansion
/// ratio. The cap applies to every frame, compressed or not. Pass `None` for
/// no limit.
///
/// # Returns
/// - `Ok(EnvelopePayload::Message(payload))` for message frames (END_STREAM bit clear)
/// - `Ok(EnvelopePayload::EndStream(payload))` for end-stream frames (END_STREAM bit set)
/// - `Err` for flags with unknown bits set, decompression failures, or payloads
///   exceeding `max_output` ([`EnvelopeError::MessageTooLarge`])
///
/// # Arguments
/// - `flags`: The envelope flags byte
/// - `payload`: The raw payload bytes from the envelope
/// - `encoding`: Compression encoding to use for decompression
/// - `max_output`: Upper bound on the decompressed payload size, or `None` for no limit
pub fn process_envelope_payload(
    flags: u8,
    payload: Bytes,
    encoding: CompressionEncoding,
    max_output: Option<usize>,
) -> Result<EnvelopePayload, EnvelopeError> {
    // Reject flags with bits outside the defined set (COMPRESSED | END_STREAM).
    const KNOWN_FLAGS: u8 = envelope_flags::COMPRESSED | envelope_flags::END_STREAM;
    if flags & !KNOWN_FLAGS != 0 {
        return Err(EnvelopeError::InvalidFlags(flags));
    }

    // COMPRESSED bit (0x01) indicates a per-frame compressed payload.
    let is_compressed = flags & envelope_flags::COMPRESSED != 0;

    // Decompress if needed, bounding output to guard against decompression
    // bombs. Uncompressed frames are capped too so `max_output` holds for
    // every flag.
    let payload = if is_compressed {
        decompress_payload(payload, encoding, max_output)?
    } else {
        check_max_output(payload, max_output)?
    };

    // EndStream bit (0x02) signals end of stream; it may be combined with the
    // COMPRESSED bit (0x03), so test the bit rather than the whole byte.
    if flags & envelope_flags::END_STREAM != 0 {
        Ok(EnvelopePayload::EndStream(payload))
    } else {
        Ok(EnvelopePayload::Message(payload))
    }
}

/// Return `payload` unchanged unless its length exceeds `max_output`.
fn check_max_output(payload: Bytes, max_output: Option<usize>) -> Result<Bytes, EnvelopeError> {
    match max_output {
        Some(limit) if payload.len() > limit => Err(EnvelopeError::MessageTooLarge { limit }),
        _ => Ok(payload),
    }
}

/// Decompress payload bytes based on encoding, bounding the output size.
fn decompress_payload(
    payload: Bytes,
    encoding: CompressionEncoding,
    max_output: Option<usize>,
) -> Result<Bytes, EnvelopeError> {
    let Some(codec) = encoding.codec() else {
        // COMPRESSED bit set but no compression negotiated: protocol error
        // (matching connect-go's envelope handling).
        return Err(EnvelopeError::MissingCompression);
    };

    codec
        .decompress_limited(&payload, max_output.unwrap_or(usize::MAX))
        .map_err(|e| match e {
            DecompressError::TooLarge { limit } => EnvelopeError::MessageTooLarge { limit },
            DecompressError::Io(e) => EnvelopeError::Decompression(e.to_string()),
        })
}

/// Compress payload bytes using the given codec.
///
/// Returns `(compressed_bytes, was_compressed)`.
pub fn compress_payload(
    payload: Bytes,
    codec: Option<&BoxedCodec>,
) -> Result<(Bytes, bool), EnvelopeError> {
    let Some(codec) = codec else {
        return Ok((payload, false)); // identity
    };

    let compressed = codec
        .compress(&payload)
        .map_err(|e| EnvelopeError::Compression(e.to_string()))?;

    Ok((compressed, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_envelope_uncompressed() {
        let payload = b"hello";
        let frame = wrap_envelope(payload, false).unwrap();

        assert_eq!(frame[0], envelope_flags::MESSAGE);
        assert_eq!(
            u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]),
            5
        );
        assert_eq!(&frame[5..], b"hello");
    }

    #[test]
    fn test_wrap_envelope_compressed() {
        let payload = b"hello";
        let frame = wrap_envelope(payload, true).unwrap();

        assert_eq!(frame[0], envelope_flags::COMPRESSED);
        assert_eq!(&frame[5..], b"hello");
    }

    #[test]
    fn test_parse_envelope_header() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let (flags, length) = parse_envelope_header(&data).unwrap();

        assert_eq!(flags, envelope_flags::MESSAGE);
        assert_eq!(length, 5);
    }

    #[test]
    fn test_parse_envelope_header_incomplete() {
        let data = [0x00, 0x00, 0x00]; // only 3 bytes
        let result = parse_envelope_header(&data);

        assert!(result.is_err());
    }

    #[test]
    fn test_process_envelope_payload_message() {
        let payload = Bytes::from_static(b"hello");
        let result = process_envelope_payload(
            envelope_flags::MESSAGE,
            payload.clone(),
            CompressionEncoding::Identity,
            None,
        )
        .unwrap();

        assert_eq!(result, EnvelopePayload::Message(payload));
    }

    #[test]
    fn test_process_envelope_payload_end_stream() {
        let payload = Bytes::from_static(b"{}");
        let result = process_envelope_payload(
            envelope_flags::END_STREAM,
            payload.clone(),
            CompressionEncoding::Identity,
            None,
        )
        .unwrap();

        assert_eq!(result, EnvelopePayload::EndStream(payload));
    }

    #[test]
    fn test_process_envelope_payload_compressed_without_negotiation() {
        // COMPRESSED bit with Identity (no negotiated compression) is a
        // protocol error, matching connect-go.
        let flags = envelope_flags::COMPRESSED;
        let err = process_envelope_payload(
            flags,
            Bytes::from_static(b"data"),
            CompressionEncoding::Identity,
            None,
        )
        .unwrap_err();

        assert!(matches!(err, EnvelopeError::MissingCompression));
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_process_envelope_payload_compressed_end_stream() {
        // Flags are a bitfield: COMPRESSED | END_STREAM (0x03) is a valid
        // end-stream frame and must not be rejected as invalid flags.
        let codec = CompressionEncoding::Gzip.codec().unwrap();
        let compressed = codec.compress(b"{}").unwrap();
        let flags = envelope_flags::COMPRESSED | envelope_flags::END_STREAM;
        let result =
            process_envelope_payload(flags, compressed, CompressionEncoding::Gzip, None).unwrap();

        assert_eq!(
            result,
            EnvelopePayload::EndStream(Bytes::from_static(b"{}"))
        );
    }

    #[test]
    fn test_process_envelope_payload_invalid_flags() {
        let payload = Bytes::from_static(b"hello");
        let result = process_envelope_payload(0xFF, payload, CompressionEncoding::Identity, None);

        assert!(result.is_err());
    }

    #[test]
    fn test_process_envelope_payload_enforces_max_output() {
        let payload = Bytes::from(vec![0u8; 100]);
        // Within limit passes.
        let result = process_envelope_payload(
            envelope_flags::MESSAGE,
            payload.clone(),
            CompressionEncoding::Identity,
            Some(100),
        )
        .unwrap();
        assert_eq!(result, EnvelopePayload::Message(payload.clone()));

        // Over limit is rejected, even without decompression.
        let err = process_envelope_payload(
            envelope_flags::MESSAGE,
            payload,
            CompressionEncoding::Identity,
            Some(99),
        )
        .unwrap_err();
        assert!(matches!(err, EnvelopeError::MessageTooLarge { limit: 99 }));
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_process_envelope_payload_rejects_bomb() {
        // 1 MiB of zeros compresses to ~1 KiB but expands far past the limit.
        let codec = CompressionEncoding::Gzip.codec().unwrap();
        let bomb = codec.compress(&vec![0u8; 1024 * 1024]).unwrap();

        let err = process_envelope_payload(
            envelope_flags::COMPRESSED,
            bomb,
            CompressionEncoding::Gzip,
            Some(64 * 1024),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            EnvelopeError::MessageTooLarge { limit } if limit == 64 * 1024
        ));
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_process_envelope_payload_decompresses_end_stream() {
        let codec = CompressionEncoding::Gzip.codec().unwrap();
        let payload = br#"{"metadata":{"x-t":["1"]}}"#;
        let compressed = codec.compress(payload).unwrap();

        let flags = envelope_flags::COMPRESSED | envelope_flags::END_STREAM;
        let result =
            process_envelope_payload(flags, compressed, CompressionEncoding::Gzip, None).unwrap();

        assert_eq!(
            result,
            EnvelopePayload::EndStream(Bytes::from_static(payload))
        );
    }

    #[test]
    fn test_compress_payload_identity() {
        let payload = Bytes::from_static(b"hello");
        let (result, compressed) = compress_payload(payload.clone(), None).unwrap();

        assert_eq!(result, payload);
        assert!(!compressed);
    }
}
