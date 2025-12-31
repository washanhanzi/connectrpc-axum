//! Request and response pipelines for Connect RPC.
//!
//! Pipelines handle the full request/response lifecycle:
//! - RequestPipeline: decode request (decompress, check limits, decode)
//! - ResponsePipeline: encode response (encode, compress)
//!
//! All configuration is read from Context in request extensions.
//!
//! ## Primitive Functions
//!
//! For fine-grained control, use the building blocks directly:
//!
//! ### Request primitives
//! - [`read_body`]: Read HTTP body with size limit
//! - [`decompress_bytes`]: Decompress bytes based on encoding
//! - [`decode_proto`]: Decode protobuf message
//! - [`decode_json`]: Decode JSON message
//! - [`unwrap_envelope`]: Unwrap a Connect streaming frame envelope
//!
//! ### Response primitives
//! - [`encode_proto`]: Encode protobuf message to bytes
//! - [`encode_json`]: Encode JSON message to bytes
//! - [`compress_bytes`]: Compress bytes if beneficial
//! - [`wrap_envelope`]: Wrap payload in a Connect streaming frame
//! - [`build_end_stream_frame`]: Build an EndStream frame for streaming responses

use crate::context::{
    CompressionEncoding, Context, RequestProtocol, compress, decompress, error::ContextError,
};
use crate::error::{Code, ConnectError};
use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header};
use bytes::Bytes;
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};

// ============================================================================
// Primitive Functions
// ============================================================================

/// Read HTTP body bytes with a size limit.
///
/// Returns `ResourceExhausted` error if the body exceeds `max_size`.
pub async fn read_body(body: Body, max_size: usize) -> Result<Bytes, ConnectError> {
    axum::body::to_bytes(body, max_size).await.map_err(|e| {
        ConnectError::new(
            Code::ResourceExhausted,
            format!("failed to read request body: {e}"),
        )
    })
}

/// Decompress bytes based on compression encoding.
///
/// Returns the original bytes unchanged (zero-copy) if encoding is `Identity`.
/// Returns `InvalidArgument` error if decompression fails.
pub fn decompress_bytes(
    bytes: Bytes,
    encoding: CompressionEncoding,
) -> Result<Bytes, ConnectError> {
    if encoding == CompressionEncoding::Identity {
        return Ok(bytes); // Zero-copy passthrough
    }
    decompress(&bytes, encoding)
        .map(Bytes::from)
        .map_err(|e| ConnectError::new(Code::InvalidArgument, format!("decompression failed: {e}")))
}

/// Decode a protobuf message from bytes.
///
/// Returns `InvalidArgument` error if decoding fails.
pub fn decode_proto<T>(bytes: &[u8]) -> Result<T, ConnectError>
where
    T: Message + Default,
{
    T::decode(bytes).map_err(|e| {
        ConnectError::new(
            Code::InvalidArgument,
            format!("failed to decode protobuf message: {e}"),
        )
    })
}

/// Decode a JSON message from bytes.
///
/// Returns `InvalidArgument` error if decoding fails.
pub fn decode_json<T>(bytes: &[u8]) -> Result<T, ConnectError>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(bytes).map_err(|e| {
        ConnectError::new(
            Code::InvalidArgument,
            format!("failed to decode JSON message: {e}"),
        )
    })
}

/// Unwrap a single Connect envelope frame.
///
/// Frame format: `[flags:1][length:4][payload:length]`
///
/// Returns the payload bytes. Validates that flags indicate a regular message
/// (0x00) and that the frame is complete.
///
/// # Errors
/// - `InvalidArgument` if the envelope is incomplete or malformed
/// - `InvalidArgument` if flags indicate end-of-stream (0x02) or unknown flags
pub fn unwrap_envelope(bytes: &[u8]) -> Result<Bytes, ConnectError> {
    if bytes.len() < 5 {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "protocol error: incomplete envelope",
        ));
    }

    let flags = bytes[0];
    let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;

    // Connect streaming: flag 0x00 = message, 0x02 = end-stream
    if flags == 0x02 {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unexpected EndStreamResponse in request",
        ));
    } else if flags != 0x00 {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!("invalid Connect frame flags: 0x{:02x}", flags),
        ));
    }

    // Validate frame length
    let expected_len = 5 + length;
    if bytes.len() > expected_len {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "frame has {} unexpected trailing bytes",
                bytes.len() - expected_len
            ),
        ));
    } else if bytes.len() < expected_len {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "incomplete frame: expected {} bytes, got {}",
                expected_len,
                bytes.len()
            ),
        ));
    }

    Ok(Bytes::copy_from_slice(&bytes[5..expected_len]))
}

// ============================================================================
// Response Primitive Functions
// ============================================================================

/// Encode a protobuf message to bytes.
pub fn encode_proto<T>(message: &T) -> Vec<u8>
where
    T: Message,
{
    message.encode_to_vec()
}

/// Encode a message to JSON bytes.
///
/// Returns `Internal` error if serialization fails.
pub fn encode_json<T>(message: &T) -> Result<Vec<u8>, ConnectError>
where
    T: Serialize,
{
    serde_json::to_vec(message).map_err(|e| {
        ConnectError::new(Code::Internal, format!("failed to encode JSON message: {e}"))
    })
}

/// Compress bytes if beneficial.
///
/// Returns a tuple of (data, was_compressed).
/// Compression is applied only if:
/// - encoding is not Identity
/// - data length >= min_bytes threshold
///
/// Falls back to uncompressed data on compression error.
pub fn compress_bytes(
    data: Vec<u8>,
    encoding: CompressionEncoding,
    min_bytes: usize,
) -> (Vec<u8>, bool) {
    if encoding == CompressionEncoding::Identity || data.len() < min_bytes {
        return (data, false);
    }

    match compress(&data, encoding) {
        Ok(compressed) => (compressed, true),
        Err(_) => (data, false), // Fall back to uncompressed on error
    }
}

/// Connect streaming envelope flags.
pub mod envelope_flags {
    /// Regular message (uncompressed).
    pub const MESSAGE: u8 = 0x00;
    /// Compressed message.
    pub const COMPRESSED: u8 = 0x01;
    /// End of stream.
    pub const END_STREAM: u8 = 0x02;
}

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

    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(flags);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Build an EndStream frame for streaming responses.
///
/// Frame format: `[flags=0x02][length:4][json_payload]`
///
/// # Arguments
/// - `error`: Optional error to include in the EndStream message
pub fn build_end_stream_frame(error: Option<&ConnectError>) -> Vec<u8> {
    let json_payload = if let Some(err) = error {
        serde_json::json!({ "error": err })
    } else {
        serde_json::json!({})
    };

    // Serializing {} or {"error": ...} cannot fail in serde_json
    let payload = serde_json::to_vec(&json_payload).unwrap_or_else(|_| b"{}".to_vec());

    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(envelope_flags::END_STREAM);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame
}

/// Set Connect-Content-Encoding header for streaming responses.
///
/// For streaming responses, the Connect protocol uses `connect-content-encoding`
/// instead of the standard `content-encoding` header.
/// Only adds the header if encoding is not Identity.
pub fn set_connect_content_encoding(
    mut builder: axum::http::response::Builder,
    encoding: CompressionEncoding,
) -> axum::http::response::Builder {
    if encoding != CompressionEncoding::Identity {
        builder = builder.header("connect-content-encoding", encoding.as_str());
    }
    builder
}

// ============================================================================
// RequestPipeline
// ============================================================================

/// Request pipeline - decodes incoming request messages.
///
/// Handles: body reading, decompression, size limits, protocol decoding.
pub struct RequestPipeline;

impl RequestPipeline {
    /// Decode request message from HTTP request.
    ///
    /// Reads Context from extensions, reads body, decompresses, decodes.
    /// This is a convenience method that composes the primitive functions.
    pub async fn decode<T>(req: Request<Body>) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        // 1. Get context from extensions (injected by layer)
        let ctx = req.extensions().get::<Context>().cloned().ok_or_else(|| {
            ContextError::internal(RequestProtocol::Unknown, "missing request context")
        })?;

        // 2. Read body bytes with size limit
        let max_size = ctx.limits.max_message_size().unwrap_or(usize::MAX);
        let body = read_body(req.into_body(), max_size)
            .await
            .map_err(|e| ContextError::new(ctx.protocol, e))?;

        // 3. Decode using the body
        Self::decode_bytes(&ctx, body)
    }

    /// Decode from raw bytes (for use when body is already read).
    ///
    /// Composes: decompress → check_size → decode
    pub fn decode_bytes<T>(ctx: &Context, body: Bytes) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        // 1. Decompress if needed
        let body = decompress_bytes(body, ctx.compression.request_encoding)
            .map_err(|e| ContextError::new(ctx.protocol, e))?;

        // 2. Check decompressed size
        ctx.limits.check_size(body.len()).map_err(|msg| {
            ContextError::new(
                ctx.protocol,
                ConnectError::new(Code::ResourceExhausted, msg),
            )
        })?;

        // 3. Decode based on protocol
        if ctx.protocol.is_proto() {
            decode_proto(&body).map_err(|e| ContextError::new(ctx.protocol, e))
        } else {
            decode_json(&body).map_err(|e| ContextError::new(ctx.protocol, e))
        }
    }
}

// ============================================================================
// ResponsePipeline
// ============================================================================

/// Response pipeline - encodes outgoing response messages.
///
/// Handles: protocol encoding, compression, HTTP response building.
pub struct ResponsePipeline;

impl ResponsePipeline {
    /// Encode response message to HTTP response.
    ///
    /// Reads Context from request extensions.
    pub fn encode<T>(req: &Request<Body>, message: &T) -> Result<Response<Body>, ContextError>
    where
        T: Message + Serialize,
    {
        let ctx = req.extensions().get::<Context>().ok_or_else(|| {
            ContextError::internal(RequestProtocol::Unknown, "missing request context")
        })?;

        Self::encode_with_context(ctx, message)
    }

    /// Encode with explicit context (when request not available).
    pub fn encode_with_context<T>(
        ctx: &Context,
        message: &T,
    ) -> Result<Response<Body>, ContextError>
    where
        T: Message + Serialize,
    {
        // 1. Encode based on protocol
        let body = if ctx.protocol.is_proto() {
            encode_proto(message)
        } else {
            encode_json(message).map_err(|e| ContextError::new(ctx.protocol, e))?
        };

        // 2. Compress if beneficial
        let compression = &ctx.compression;
        let (body, was_compressed) = compress_bytes(
            body,
            compression.response_encoding,
            compression.min_compress_bytes,
        );

        // 3. Build HTTP response
        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctx.protocol.response_content_type());

        if was_compressed {
            builder = builder.header(
                header::CONTENT_ENCODING,
                compression.response_encoding.as_str(),
            );
        }

        builder
            .body(Body::from(body))
            .map_err(|e| ContextError::internal(ctx.protocol, e.to_string()))
    }
}
