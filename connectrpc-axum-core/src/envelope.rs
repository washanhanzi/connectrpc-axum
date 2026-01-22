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
use crate::error::{Code, ConnectError};

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
pub fn parse_envelope_header(data: &[u8]) -> Result<(u8, u32), ConnectError> {
    if data.len() < ENVELOPE_HEADER_SIZE {
        return Err(ConnectError::Protocol(format!(
            "incomplete envelope header: expected {} bytes, got {}",
            ENVELOPE_HEADER_SIZE,
            data.len()
        )));
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
/// # Returns
/// - `Ok(Some(payload))` for message frames (flags 0x00 or 0x01)
/// - `Ok(None)` for end-stream frames (flag 0x02)
/// - `Err` for invalid/unknown flags
///
/// # Arguments
/// - `flags`: The envelope flags byte
/// - `payload`: The raw payload bytes from the envelope
/// - `encoding`: Compression encoding to use for decompression
pub fn process_envelope_payload(
    flags: u8,
    payload: Bytes,
    encoding: CompressionEncoding,
) -> Result<Option<Bytes>, ConnectError> {
    // EndStream frame (flags = 0x02) signals end of stream
    if flags == envelope_flags::END_STREAM {
        return Ok(None);
    }

    // Validate message flags: 0x00 = uncompressed, 0x01 = compressed
    let is_compressed = flags == envelope_flags::COMPRESSED;
    if flags != envelope_flags::MESSAGE && !is_compressed {
        return Err(ConnectError::Protocol(format!(
            "invalid Connect frame flags: 0x{:02x}",
            flags
        )));
    }

    // Decompress if needed
    let payload = if is_compressed {
        decompress_payload(payload, encoding)?
    } else {
        payload
    };

    Ok(Some(payload))
}

/// Decompress payload bytes based on encoding.
fn decompress_payload(payload: Bytes, encoding: CompressionEncoding) -> Result<Bytes, ConnectError> {
    let Some(codec) = encoding.codec() else {
        return Ok(payload); // identity: passthrough
    };

    codec.decompress(&payload).map_err(|e| {
        ConnectError::new(Code::InvalidArgument, format!("decompression failed: {e}"))
    })
}

/// Compress payload bytes based on encoding.
///
/// Returns `(compressed_bytes, was_compressed)`.
pub fn compress_payload(
    payload: Bytes,
    codec: Option<&BoxedCodec>,
) -> Result<(Bytes, bool), ConnectError> {
    let Some(codec) = codec else {
        return Ok((payload, false)); // identity
    };

    let compressed = codec
        .compress(&payload)
        .map_err(|e| ConnectError::new(Code::Internal, format!("compression failed: {e}")))?;

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
    fn test_process_envelope_payload_invalid_flags() {
        let payload = Bytes::from_static(b"hello");
        let result =
            process_envelope_payload(0xFF, payload, CompressionEncoding::Identity);

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
