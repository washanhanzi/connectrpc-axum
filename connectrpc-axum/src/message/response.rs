//! Response types for Connect.
use crate::context::{CompressionEncoding, ConnectContext};
use crate::error::{
    ConnectError, internal_error_end_stream_frame, internal_error_response,
    internal_error_streaming_response,
};
use crate::pipeline::{
    build_end_stream_frame, compress_bytes, encode_json, encode_proto,
    set_connect_content_encoding, wrap_envelope,
};
use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use futures::Stream;
use prost::Message;
use serde::Serialize;

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
                let err = ConnectError::new(crate::error::Code::ResourceExhausted, msg);
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
            ctx.compression.min_compress_bytes,
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
                let err = ConnectError::new(crate::error::Code::ResourceExhausted, msg);
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

        eprintln!(
            "[DEBUG] StreamBody::into_response_with_context: send_max_bytes={:?}",
            ctx.limits.get_send_max_bytes()
        );

        self.into_response_with_context_inner(
            ctx.protocol.is_proto(),
            ctx.protocol.streaming_response_content_type(),
            response_encoding,
            ctx.compression.min_compress_bytes,
            ctx.limits.get_send_max_bytes(),
        )
    }

    fn into_response_with_context_inner(
        self,
        use_proto: bool,
        content_type: &'static str,
        response_encoding: CompressionEncoding,
        min_compress_bytes: usize,
        send_max_bytes: Option<usize>,
    ) -> Response {
        use crate::error::Code;
        use futures::StreamExt;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

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
                        match compress_bytes(payload, response_encoding, min_compress_bytes) {
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
