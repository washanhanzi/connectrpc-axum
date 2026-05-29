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

use crate::codec::BoxedCodec;
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

/// Minimum envelope header size (flags + length).
pub const ENVELOPE_HEADER_SIZE: usize = 5;

/// Wrap payload in a Connect streaming frame envelope.
///
/// Frame format: `[flags:1][length:4][payload]`
///
/// # Arguments
/// - `payload`: The message bytes to wrap
/// - `compressed`: Whether the payload is compressed (sets flag 0x01)
pub fn wrap_envelope(payload: &[u8], compressed: bool) -> Vec<u8> {
    let flags = if compressed {
        envelope_flags::COMPRESSED
    } else {
        envelope_flags::MESSAGE
    };

    let mut frame = Vec::with_capacity(ENVELOPE_HEADER_SIZE + payload.len());
    frame.push(flags);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
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

/// Process envelope payload based on flags, with optional decompression.
///
/// Given the flags byte and payload bytes from an envelope, validates the flags
/// and decompresses the payload if needed.
///
/// Flags are a bitfield per the Connect spec, so they are matched with bitwise
/// masks rather than exact equality: a compressed end-stream frame is `0x03`
/// (`COMPRESSED | END_STREAM`), not a distinct value.
///
/// # Returns
/// - `Ok(Some(payload))` for message frames (END_STREAM bit clear)
/// - `Ok(None)` for end-stream frames (END_STREAM bit set, e.g. 0x02 or 0x03)
/// - `Err` for flags with unknown bits set
///
/// # Arguments
/// - `flags`: The envelope flags byte
/// - `payload`: The raw payload bytes from the envelope
/// - `encoding`: Compression encoding to use for decompression
pub fn process_envelope_payload(
    flags: u8,
    payload: Bytes,
    encoding: CompressionEncoding,
) -> Result<Option<Bytes>, EnvelopeError> {
    // Reject flags with bits outside the defined set (COMPRESSED | END_STREAM).
    const KNOWN_FLAGS: u8 = envelope_flags::COMPRESSED | envelope_flags::END_STREAM;
    if flags & !KNOWN_FLAGS != 0 {
        return Err(EnvelopeError::InvalidFlags(flags));
    }

    // EndStream bit (0x02) signals end of stream; it may be combined with the
    // COMPRESSED bit (0x03), so test the bit rather than the whole byte.
    if flags & envelope_flags::END_STREAM != 0 {
        return Ok(None);
    }

    // COMPRESSED bit (0x01) indicates a per-frame compressed payload.
    let is_compressed = flags & envelope_flags::COMPRESSED != 0;

    // Decompress if needed
    let payload = if is_compressed {
        decompress_payload(payload, encoding)?
    } else {
        payload
    };

    Ok(Some(payload))
}

/// Decompress payload bytes based on encoding.
fn decompress_payload(
    payload: Bytes,
    encoding: CompressionEncoding,
) -> Result<Bytes, EnvelopeError> {
    let Some(codec) = encoding.codec() else {
        return Ok(payload); // identity: passthrough
    };

    codec
        .decompress(&payload)
        .map_err(|e| EnvelopeError::Decompression(e.to_string()))
}

/// Compress payload bytes based on encoding.
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
        let frame = wrap_envelope(payload, false);

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
        let frame = wrap_envelope(payload, true);

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
        )
        .unwrap();

        assert_eq!(result, Some(payload));
    }

    #[test]
    fn test_process_envelope_payload_end_stream() {
        let payload = Bytes::from_static(b"{}");
        let result = process_envelope_payload(
            envelope_flags::END_STREAM,
            payload,
            CompressionEncoding::Identity,
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_process_envelope_payload_compressed_end_stream() {
        // Flags are a bitfield: COMPRESSED | END_STREAM (0x03) is a valid
        // end-stream frame and must not be rejected as invalid flags.
        let flags = envelope_flags::COMPRESSED | envelope_flags::END_STREAM;
        let result = process_envelope_payload(
            flags,
            Bytes::from_static(b"{}"),
            CompressionEncoding::Identity,
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_process_envelope_payload_invalid_flags() {
        let payload = Bytes::from_static(b"hello");
        let result = process_envelope_payload(0xFF, payload, CompressionEncoding::Identity);

        assert!(result.is_err());
    }

    #[test]
    fn test_compress_payload_identity() {
        let payload = Bytes::from_static(b"hello");
        let (result, compressed) = compress_payload(payload.clone(), None).unwrap();

        assert_eq!(result, payload);
        assert!(!compressed);
    }
}
