//! Response types for Connect.
//!
//! This module provides response encoding primitives for Connect RPC.
//!
//! ## Primitive Functions
//!
//! - [`encode_proto`]: Encode protobuf message to bytes
//! - [`encode_json`]: Encode JSON message to bytes
//! - [`compress_bytes`]: Compress bytes if beneficial
//! - [`wrap_envelope`]: Wrap payload in a Connect streaming frame
//! - [`set_connect_content_encoding`]: Set Connect-Content-Encoding header
use crate::context::{CompressionConfig, CompressionEncoding, ConnectContext};
use crate::message::error::{
    Code, ConnectError, build_end_stream_frame, internal_error_end_stream_frame,
    internal_error_response, internal_error_streaming_response,
};
use crate::message::request::envelope_flags;
use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use futures::Stream;
use prost::Message;
use serde::Serialize;

// ============================================================================
// Primitive Encode Functions
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
        ConnectError::new(
            Code::Internal,
            format!("failed to encode JSON message: {e}"),
        )
    })
}

/// Compress bytes if beneficial.
///
/// Returns a tuple of (data, was_compressed).
/// Compression is applied only if:
/// - encoding is not Identity
/// - data length >= min_bytes threshold
///
/// Returns an error if compression fails (matching connect-go behavior).
pub fn compress_bytes(
    data: Bytes,
    encoding: CompressionEncoding,
    config: &CompressionConfig,
) -> Result<(Bytes, bool), ConnectError> {
    let Some(codec) = encoding.codec_with_level(config.level) else {
        return Ok((data, false));
    };

    if data.len() < config.min_bytes {
        return Ok((data, false));
    }

    match codec.compress(&data) {
        Ok(compressed) => Ok((compressed, true)),
        Err(e) => Err(ConnectError::new(Code::Internal, format!("compress: {e}"))),
    }
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

/// Response wrapper for Connect RPC handlers.
///
/// A simple tuple struct that wraps the response value.
/// Protocol encoding is handled at the framework level, not stored in the response.
#[derive(Debug, Clone)]
pub struct ConnectResponse<T>(pub T);

impl<T> ConnectResponse<T> {
    /// Create a new ConnectResponse wrapping the given value.
    pub fn new(inner: T) -> Self {
        Self(inner)
    }

    /// Extract the inner value from the ConnectResponse wrapper.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> ConnectResponse<T>
where
    T: Message + Serialize,
{
    /// Encode the response using pipeline context.
    /// This is called by handler wrappers for unary responses.
    pub(crate) fn into_response_with_context(self, ctx: &ConnectContext) -> Response {
        // 1. Encode based on protocol
        let body: Bytes = if ctx.protocol.is_proto() {
            Bytes::from(encode_proto(&self.0))
        } else {
            match encode_json(&self.0) {
                Ok(bytes) => Bytes::from(bytes),
                Err(_) => return internal_error_response(ctx.protocol.error_content_type()),
            }
        };

        // 2. Check send size limit (following connect-go behavior)
        // Note: For unary RPCs, Tower's CompressionLayer handles HTTP body compression,
        // so we check the uncompressed size here. Tower will compress the response body.
        if let Some(max) = ctx.limits.get_send_max_bytes() {
            if body.len() > max {
                let msg = format!("message size {} exceeds sendMaxBytes {}", body.len(), max);
                let err = ConnectError::new(crate::message::error::Code::ResourceExhausted, msg);
                return err.into_response_with_protocol(ctx.protocol);
            }
        }

        // 3. Build HTTP response
        // Note: Compression is handled by Tower's CompressionLayer for unary RPCs.
        // We don't set Content-Encoding here; Tower will add it based on Accept-Encoding.
        let builder = Response::builder().status(StatusCode::OK).header(
            header::CONTENT_TYPE,
            HeaderValue::from_static(ctx.protocol.response_content_type()),
        );

        builder
            .body(Body::from(body))
            .unwrap_or_else(|_| internal_error_response(ctx.protocol.error_content_type()))
    }

    /// Encode the response as a streaming response (single message frame + EndStreamResponse).
    ///
    /// This is used for client streaming RPCs where the response is a single message
    /// but must be sent in streaming format with framing.
    pub(crate) fn into_streaming_response_with_context(self, ctx: &ConnectContext) -> Response {
        let content_type = ctx.protocol.streaming_response_content_type();

        // Get envelope compression settings (for streaming, this should be Some)
        let response_encoding = ctx
            .compression
            .envelope
            .map(|e| e.response)
            .unwrap_or(CompressionEncoding::Identity);

        // 1. Encode the message
        let payload: Bytes = if ctx.protocol.is_proto() {
            Bytes::from(encode_proto(&self.0))
        } else {
            match encode_json(&self.0) {
                Ok(bytes) => Bytes::from(bytes),
                Err(_) => return internal_error_streaming_response(content_type),
            }
        };

        // 2. Compress if beneficial (per-envelope compression for streaming)
        let (data, compressed) = match compress_bytes(
            payload,
            response_encoding,
            &ctx.compression.config,
        ) {
            Ok(result) => result,
            Err(_) => return internal_error_streaming_response(content_type),
        };

        // 3. Check send size limit (following connect-go behavior)
        if let Some(max) = ctx.limits.get_send_max_bytes() {
            if data.len() > max {
                let msg = if compressed {
                    format!(
                        "compressed message size {} exceeds sendMaxBytes {}",
                        data.len(),
                        max
                    )
                } else {
                    format!("message size {} exceeds sendMaxBytes {}", data.len(), max)
                };
                let err = ConnectError::new(crate::message::error::Code::ResourceExhausted, msg);
                // For streaming protocols, errors are returned as EndStream frames
                return err.into_response_with_protocol(ctx.protocol);
            }
        }

        // 4. Build message frame
        let message_frame = wrap_envelope(&data, compressed);

        // 5. Build EndStream frame
        let end_stream_frame = build_end_stream_frame(None, None);

        // 6. Combine frames
        let mut body = message_frame;
        body.extend_from_slice(&end_stream_frame);

        let builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        let builder = set_connect_content_encoding(builder, response_encoding);

        builder
            .body(Body::from(body))
            .unwrap_or_else(|_| internal_error_streaming_response(content_type))
    }
}

// So that `Result<ConnectResponse<T>, ConnectError>` can be returned from handlers.
impl<T> From<ConnectResponse<T>> for Result<ConnectResponse<T>, ConnectError> {
    fn from(res: ConnectResponse<T>) -> Self {
        Ok(res)
    }
}

// ============================================================================
// Streaming Response Support
// ============================================================================

/// Wrapper type for streaming response bodies.
/// This allows us to use `ConnectResponse<StreamBody<S>>` for server streaming
/// without conflicting with the single-message `ConnectResponse<T>` implementation.
#[derive(Debug)]
pub struct StreamBody<S> {
    stream: S,
}

impl<S> StreamBody<S> {
    /// Create a new StreamBody wrapping a stream.
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    /// Extract the underlying stream.
    pub fn into_inner(self) -> S {
        self.stream
    }
}

impl<S, T> ConnectResponse<StreamBody<S>>
where
    S: Stream<Item = Result<T, ConnectError>> + Send + 'static,
    T: Message + Serialize + Send + 'static,
{
    /// Encode the streaming response using pipeline context.
    /// This is called by handler wrappers for streaming responses with compression support.
    pub(crate) fn into_response_with_context(self, ctx: &ConnectContext) -> Response {
        // Get envelope compression settings (for streaming, this should be Some)
        let response_encoding = ctx
            .compression
            .envelope
            .map(|e| e.response)
            .unwrap_or(CompressionEncoding::Identity);

        self.into_response_with_context_inner(
            ctx.protocol.is_proto(),
            ctx.protocol.streaming_response_content_type(),
            response_encoding,
            &ctx.compression.config,
            ctx.limits.get_send_max_bytes(),
        )
    }

    fn into_response_with_context_inner(
        self,
        use_proto: bool,
        content_type: &'static str,
        response_encoding: CompressionEncoding,
        config: &CompressionConfig,
        send_max_bytes: Option<usize>,
    ) -> Response {
        use crate::message::error::Code;
        use futures::StreamExt;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        // Copy config for use in closure (CompressionConfig is Copy)
        let config = *config;

        // Track if an error was sent (for EndStream handling)
        let error_sent = Arc::new(AtomicBool::new(false));
        let error_sent_clone = error_sent.clone();

        let body_stream = self
            .0
            .stream
            .map(move |result| match result {
                Ok(msg) => {
                    // 1. Encode based on protocol
                    let payload: Bytes = if use_proto {
                        Bytes::from(encode_proto(&msg))
                    } else {
                        match encode_json(&msg) {
                            Ok(bytes) => Bytes::from(bytes),
                            Err(_) => {
                                return (Bytes::from(internal_error_end_stream_frame()), true);
                            }
                        }
                    };

                    // 2. Compress if beneficial (per-message compression)
                    let (data, compressed) =
                        match compress_bytes(payload, response_encoding, &config) {
                            Ok(result) => result,
                            Err(_) => {
                                return (Bytes::from(internal_error_end_stream_frame()), true);
                            }
                        };

                    // 3. Check send size limit (following connect-go behavior)
                    eprintln!(
                        "[DEBUG] Streaming message: size={}, send_max_bytes={:?}",
                        data.len(),
                        send_max_bytes
                    );
                    if let Some(max) = send_max_bytes {
                        if data.len() > max {
                            let msg = if compressed {
                                format!(
                                    "compressed message size {} exceeds sendMaxBytes {}",
                                    data.len(),
                                    max
                                )
                            } else {
                                format!("message size {} exceeds sendMaxBytes {}", data.len(), max)
                            };
                            let err = ConnectError::new(Code::ResourceExhausted, msg);
                            let frame = build_end_stream_frame(Some(&err), None);
                            return (Bytes::from(frame), true);
                        }
                    }

                    // 4. Wrap in envelope with correct flags
                    let frame = wrap_envelope(&data, compressed);
                    (Bytes::from(frame), false)
                }
                Err(err) => {
                    // Send Error EndStreamResponse (includes error metadata in the frame)
                    let frame = build_end_stream_frame(Some(&err), None);
                    (Bytes::from(frame), true)
                }
            })
            // Take all messages, stop after error
            .scan(false, move |error_seen, (bytes, is_error)| {
                if *error_seen {
                    futures::future::ready(None)
                } else if is_error {
                    *error_seen = true;
                    error_sent.store(true, Ordering::SeqCst);
                    futures::future::ready(Some(bytes))
                } else {
                    futures::future::ready(Some(bytes))
                }
            })
            // Append success EndStreamResponse if no error was sent
            .chain(
                futures::stream::once(async move {
                    if error_sent_clone.load(Ordering::SeqCst) {
                        None
                    } else {
                        Some(Bytes::from(build_end_stream_frame(None, None)))
                    }
                })
                .filter_map(|x| async { x }),
            )
            // Wrap in Result for Body::from_stream
            .map(|bytes| Ok::<_, std::convert::Infallible>(bytes));

        let body = Body::from_stream(body_stream);

        let builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        let builder = set_connect_content_encoding(builder, response_encoding);

        builder
            .body(body)
            .unwrap_or_else(|_| internal_error_streaming_response(content_type))
    }
}

// Note: IntoResponse for Result<ConnectResponse<T>, ConnectError> is implemented by Axum's default implementation

// ============================================================================
// ResponsePipeline
// ============================================================================

use crate::context::error::ContextError;
use crate::message::request::get_context_or_default;

/// Response pipeline - encodes outgoing response messages.
///
/// Handles: protocol encoding, compression, HTTP response building.
pub struct ResponsePipeline;

impl ResponsePipeline {
    /// Encode response message to HTTP response.
    ///
    /// Reads Context from request extensions.
    pub fn encode<T>(req: &axum::http::Request<Body>, message: &T) -> Result<Response<Body>, ContextError>
    where
        T: Message + Serialize,
    {
        // Get context (with fallback to default if layer is missing)
        let ctx = get_context_or_default(req);

        Self::encode_with_context(&ctx, message)
    }

    /// Encode with explicit context (when request not available).
    ///
    /// Note: For unary RPCs, compression is handled by Tower's CompressionLayer.
    /// This function only encodes the message, not compresses it.
    pub fn encode_with_context<T>(
        ctx: &ConnectContext,
        message: &T,
    ) -> Result<Response<Body>, ContextError>
    where
        T: Message + Serialize,
    {
        // 1. Encode based on protocol
        let body: Bytes = if ctx.protocol.is_proto() {
            Bytes::from(encode_proto(message))
        } else {
            Bytes::from(encode_json(message).map_err(|e| ContextError::new(ctx.protocol, e))?)
        };

        // 2. Build HTTP response (compression handled by Tower's CompressionLayer)
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctx.protocol.response_content_type())
            .body(Body::from(body))
            .map_err(|e| ContextError::internal(ctx.protocol, e.to_string()))
    }
}
