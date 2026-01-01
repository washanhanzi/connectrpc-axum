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
use axum::http::{HeaderMap, Request, Response, StatusCode, header};
use bytes::Bytes;
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;

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

// ============================================================================
// EndStream Metadata Support
// ============================================================================

/// Check if a header key is a protocol header that should be filtered from metadata.
///
/// Protocol headers are internal to HTTP/Connect/gRPC and should not be included
/// in the metadata field of EndStream messages.
///
/// Based on connect-go's `protocolHeaders` map in `header.go`.
fn is_protocol_header(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    matches!(
        k.as_str(),
        "content-type"
            | "content-length"
            | "content-encoding"
            | "host"
            | "user-agent"
            | "trailer"
            | "date"
    ) || k.starts_with("connect-")
        || k.starts_with("grpc-")
        || k.starts_with("trailer-")
}

/// Metadata wrapper for EndStream messages.
///
/// Serializes HTTP headers to Connect protocol metadata format:
/// - Keys map to arrays of string values
/// - Binary headers (keys ending in `-bin`) have base64-encoded values
/// - Protocol headers are filtered out
#[derive(Debug, Default)]
pub struct Metadata(HashMap<String, Vec<String>>);

impl Metadata {
    /// Create Metadata from a HeaderMap, filtering protocol headers
    /// and encoding binary values.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();

        for (key, value) in headers.iter() {
            let key_str = key.as_str();

            // Skip protocol headers
            if is_protocol_header(key_str) {
                continue;
            }

            let values = map.entry(key_str.to_string()).or_default();

            // For -bin headers, values are already base64-encoded per Connect/gRPC convention.
            // Just convert to string (no re-encoding needed).
            // For regular headers, convert to UTF-8 string.
            if let Ok(v) = value.to_str() {
                values.push(v.to_string());
            }
            // Skip non-UTF8 values (shouldn't happen with valid HTTP headers)
        }

        Metadata(map)
    }

    /// Merge headers from another HeaderMap into this metadata.
    ///
    /// Used to merge error metadata into response trailers, following
    /// connect-go's `mergeNonProtocolHeaders` behavior.
    pub fn merge_headers(&mut self, headers: &HeaderMap) {
        for (key, value) in headers.iter() {
            let key_str = key.as_str();

            if is_protocol_header(key_str) {
                continue;
            }

            let values = self.0.entry(key_str.to_string()).or_default();

            // For -bin headers, values are already base64-encoded per Connect/gRPC convention.
            if let Ok(v) = value.to_str() {
                values.push(v.to_string());
            }
        }
    }

    /// Check if metadata is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Metadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// Build an EndStream frame for streaming responses.
///
/// Frame format: `[flags=0x02][length:4][json_payload]`
///
/// The JSON payload follows the Connect protocol specification:
/// ```json
/// {
///   "error": { "code": "...", "message": "...", "details": [...] },
///   "metadata": { "key": ["value1", "value2"] }
/// }
/// ```
/// Both fields are optional and omitted when empty/None.
///
/// # Arguments
/// - `error`: Optional error to include in the EndStream message
/// - `trailers`: Optional response trailers to include as metadata
///
/// # Metadata Handling
/// - Protocol headers (Content-Type, Connect-*, gRPC-*, etc.) are filtered
/// - Binary headers (keys ending in `-bin`) have values base64-encoded (unpadded)
/// - Error metadata is merged into trailers (following connect-go behavior)
pub fn build_end_stream_frame(error: Option<&ConnectError>, trailers: Option<&HeaderMap>) -> Vec<u8> {
    // Helper struct for JSON serialization
    #[derive(Serialize)]
    struct EndStreamMessage<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a ConnectError>,
        #[serde(skip_serializing_if = "Metadata::is_empty")]
        metadata: Metadata,
    }

    // Start with trailers if provided
    let mut metadata = trailers.map(Metadata::from_headers).unwrap_or_default();

    // Merge error metadata into trailers (like connect-go does)
    if let Some(err) = error {
        metadata.merge_headers(err.meta());
    }

    let msg = EndStreamMessage { error, metadata };
    let payload = serde_json::to_vec(&msg).unwrap_or_else(|_| b"{}".to_vec());

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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_is_protocol_header_filters_http_headers() {
        assert!(is_protocol_header("Content-Type"));
        assert!(is_protocol_header("content-type"));
        assert!(is_protocol_header("Content-Length"));
        assert!(is_protocol_header("Content-Encoding"));
        assert!(is_protocol_header("Host"));
        assert!(is_protocol_header("User-Agent"));
        assert!(is_protocol_header("Trailer"));
        assert!(is_protocol_header("Date"));
    }

    #[test]
    fn test_is_protocol_header_filters_connect_headers() {
        assert!(is_protocol_header("Connect-Timeout-Ms"));
        assert!(is_protocol_header("connect-timeout-ms"));
        assert!(is_protocol_header("Connect-Accept-Encoding"));
        assert!(is_protocol_header("Connect-Content-Encoding"));
        assert!(is_protocol_header("Connect-Protocol-Version"));
    }

    #[test]
    fn test_is_protocol_header_filters_grpc_headers() {
        assert!(is_protocol_header("Grpc-Status"));
        assert!(is_protocol_header("grpc-status"));
        assert!(is_protocol_header("Grpc-Message"));
        assert!(is_protocol_header("Grpc-Status-Details-Bin"));
    }

    #[test]
    fn test_is_protocol_header_filters_trailer_prefix() {
        assert!(is_protocol_header("Trailer-Custom"));
        assert!(is_protocol_header("trailer-custom"));
    }

    #[test]
    fn test_is_protocol_header_allows_custom_headers() {
        assert!(!is_protocol_header("X-Custom-Header"));
        assert!(!is_protocol_header("x-request-id"));
        assert!(!is_protocol_header("Authorization"));
        assert!(!is_protocol_header("x-custom-bin"));
    }

    #[test]
    fn test_metadata_from_headers_filters_protocol_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("x-custom", HeaderValue::from_static("value"));
        headers.insert("connect-timeout-ms", HeaderValue::from_static("5000"));
        headers.insert("grpc-status", HeaderValue::from_static("0"));

        let metadata = Metadata::from_headers(&headers);

        assert!(!metadata.0.contains_key("content-type"));
        assert!(!metadata.0.contains_key("connect-timeout-ms"));
        assert!(!metadata.0.contains_key("grpc-status"));
        assert!(metadata.0.contains_key("x-custom"));
        assert_eq!(metadata.0.get("x-custom"), Some(&vec!["value".to_string()]));
    }

    #[test]
    fn test_metadata_preserves_binary_header_values() {
        let mut headers = HeaderMap::new();
        // Binary headers are already base64-encoded per Connect/gRPC convention
        // base64 of [0x00, 0x01, 0x02] without padding is "AAEC"
        headers.insert("x-binary-bin", HeaderValue::from_static("AAEC"));

        let metadata = Metadata::from_headers(&headers);

        // Value should be passed through as-is (no re-encoding)
        assert_eq!(
            metadata.0.get("x-binary-bin"),
            Some(&vec!["AAEC".to_string()])
        );
    }

    #[test]
    fn test_metadata_handles_multi_value_headers() {
        let mut headers = HeaderMap::new();
        headers.append("x-multi", HeaderValue::from_static("value1"));
        headers.append("x-multi", HeaderValue::from_static("value2"));

        let metadata = Metadata::from_headers(&headers);

        let values = metadata.0.get("x-multi").unwrap();
        assert_eq!(values.len(), 2);
        assert!(values.contains(&"value1".to_string()));
        assert!(values.contains(&"value2".to_string()));
    }

    #[test]
    fn test_metadata_is_empty() {
        let empty = Metadata::default();
        assert!(empty.is_empty());

        let mut headers = HeaderMap::new();
        headers.insert("x-custom", HeaderValue::from_static("value"));
        let non_empty = Metadata::from_headers(&headers);
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_build_end_stream_frame_success_no_trailers() {
        let frame = build_end_stream_frame(None, None);

        // Check frame structure
        assert_eq!(frame[0], 0x02); // EndStream flag

        // Parse JSON payload
        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();

        // Should be empty object when no error and no metadata
        assert_eq!(msg, serde_json::json!({}));
    }

    #[test]
    fn test_build_end_stream_frame_with_trailers() {
        let mut trailers = HeaderMap::new();
        trailers.insert("x-request-id", HeaderValue::from_static("123"));

        let frame = build_end_stream_frame(None, Some(&trailers));

        // Check frame structure
        assert_eq!(frame[0], 0x02); // EndStream flag

        // Parse JSON payload
        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();

        // Should have metadata field
        assert!(msg.get("error").is_none());
        assert!(msg.get("metadata").is_some());
        assert_eq!(msg["metadata"]["x-request-id"], serde_json::json!(["123"]));
    }

    #[test]
    fn test_build_end_stream_frame_with_error() {
        let error = ConnectError::new(Code::Internal, "test error");

        let frame = build_end_stream_frame(Some(&error), None);

        // Check frame structure
        assert_eq!(frame[0], 0x02); // EndStream flag

        // Parse JSON payload
        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();

        // Should have error field
        assert!(msg.get("error").is_some());
        assert_eq!(msg["error"]["code"], "internal");
        assert_eq!(msg["error"]["message"], "test error");
    }

    #[test]
    fn test_build_end_stream_frame_error_metadata_merged() {
        let mut error = ConnectError::new(Code::Internal, "test error");
        error = error.with_meta("x-error-meta", "error-value");

        let frame = build_end_stream_frame(Some(&error), None);

        // Parse JSON payload
        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();

        // Error metadata should be in metadata field
        assert!(msg.get("metadata").is_some());
        assert_eq!(
            msg["metadata"]["x-error-meta"],
            serde_json::json!(["error-value"])
        );
    }

    #[test]
    fn test_build_end_stream_frame_filters_protocol_headers_from_trailers() {
        let mut trailers = HeaderMap::new();
        trailers.insert("content-type", HeaderValue::from_static("application/json"));
        trailers.insert("x-custom", HeaderValue::from_static("value"));
        trailers.insert("connect-timeout-ms", HeaderValue::from_static("5000"));

        let frame = build_end_stream_frame(None, Some(&trailers));

        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();

        // Protocol headers should be filtered
        let metadata = msg.get("metadata").unwrap();
        assert!(metadata.get("content-type").is_none());
        assert!(metadata.get("connect-timeout-ms").is_none());
        assert_eq!(metadata["x-custom"], serde_json::json!(["value"]));
    }
}
