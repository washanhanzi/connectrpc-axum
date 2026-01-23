//! Extractor for Connect requests.
//!
//! This module provides request extraction and decoding primitives for Connect RPC.
//!
//! ## Primitive Functions
//!
//! - [`read_body`]: Read HTTP body with size limit
//! - [`read_frame_bytes`]: Validate frame size against limits
//! - [`decompress_bytes`]: Decompress bytes based on encoding
//! - [`decode_proto`]: Decode protobuf message
//! - [`decode_json`]: Decode JSON message
//! - [`process_envelope_payload`]: Validate envelope flags and decompress payload
use crate::context::{CompressionEncoding, ConnectContext, MessageLimits, detect_protocol};
use crate::message::error::{Code, ConnectError};
use axum::{
    body::Body,
    extract::{FromRequest, Request},
    http::Method,
};
use bytes::{Bytes, BytesMut};
use futures::Stream;
use http_body_util::BodyExt;
use prost::Message;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};

// ============================================================================
// Primitive Decode Functions
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

/// Validate frame size against limits.
///
/// Returns `ResourceExhausted` error if bytes exceed `max_size`.
pub fn read_frame_bytes(bytes: Bytes, max_size: usize) -> Result<Bytes, ConnectError> {
    if bytes.len() > max_size {
        return Err(ConnectError::new(
            Code::ResourceExhausted,
            format!(
                "message size {} bytes exceeds maximum allowed size of {} bytes",
                bytes.len(),
                max_size
            ),
        ));
    }
    Ok(bytes)
}

/// Decompress bytes based on compression encoding.
///
/// Returns the original bytes unchanged (zero-copy) if encoding is `Identity`.
/// Returns `InvalidArgument` error if decompression fails.
pub fn decompress_bytes(
    bytes: Bytes,
    encoding: CompressionEncoding,
) -> Result<Bytes, ConnectError> {
    let Some(codec) = encoding.codec() else {
        return Ok(bytes); // identity: zero-copy passthrough
    };

    codec
        .decompress(&bytes)
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

/// Connect streaming envelope flags.
pub mod envelope_flags {
    /// Regular message (uncompressed).
    pub const MESSAGE: u8 = 0x00;
    /// Compressed message.
    pub const COMPRESSED: u8 = 0x01;
    /// End of stream.
    pub const END_STREAM: u8 = 0x02;
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
/// - `encoding`: Compression encoding to use for decompression (from `Connect-Content-Encoding`)
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
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!("invalid Connect frame flags: 0x{:02x}", flags),
        ));
    }

    // Decompress if needed
    let payload = if is_compressed {
        decompress_bytes(payload, encoding)?
    } else {
        payload
    };

    Ok(Some(payload))
}

// ============================================================================
// Context fallback helper
// ============================================================================

// Flag to ensure we only log the missing layer warning once per process
static WARNED_MISSING_LAYER: AtomicBool = AtomicBool::new(false);

/// Get context from request extensions, or create a default one if missing.
///
/// If the `ConnectLayer` middleware was not applied, this will:
/// 1. Detect the protocol from request headers (Content-Type or query params)
/// 2. Create a default context with no compression and default limits
/// 3. Log a warning (once per process) about the missing layer
pub fn get_context_or_default<B>(req: &axum::http::Request<B>) -> ConnectContext {
    if let Some(ctx) = req.extensions().get::<ConnectContext>() {
        return ctx.clone();
    }

    // Log warning once per process to avoid log spam
    if !WARNED_MISSING_LAYER.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            target: "connectrpc_axum",
            "ConnectLayer middleware not found in request extensions. \
             Using default context with protocol detected from headers. \
             For production use, add ConnectLayer to your router: \
             `.layer(ConnectLayer::new())`"
        );
    }

    // Create default context by detecting protocol from headers
    let protocol = detect_protocol(req);
    ConnectContext {
        protocol,
        ..Default::default()
    }
}

// ============================================================================
// RequestPipeline
// ============================================================================

use crate::context::error::ContextError;

/// Request pipeline - decodes incoming request messages.
///
/// Handles: body reading, decompression, size limits, protocol decoding.
pub struct RequestPipeline;

impl RequestPipeline {
    /// Decode request message from HTTP request.
    ///
    /// Reads Context from extensions, reads body, decompresses, decodes.
    /// This is a convenience method that composes the primitive functions.
    pub async fn decode<T>(req: axum::http::Request<Body>) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        let ctx = get_context_or_default(&req);
        let max_size = ctx.limits.receive_max_bytes_or_max();
        let body = read_body(req.into_body(), max_size)
            .await
            .map_err(|e| ContextError::new(ctx.protocol, e))?;

        Self::decode_bytes(&ctx, body)
    }

    /// Decode from raw bytes (for use when body is already read).
    ///
    /// Note: For unary RPCs, decompression and size checking are handled by
    /// Tower's DecompressionLayer and BridgeLayer respectively.
    pub fn decode_bytes<T>(ctx: &ConnectContext, body: Bytes) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        Self::decode_message(ctx, &body)
    }

    /// Decode from enveloped bytes (for streaming-style unary requests).
    ///
    /// Used when Content-Type is `application/connect+json` or `application/connect+proto`.
    /// These use envelope framing even for unary requests.
    ///
    /// Handles per-envelope compression: frames with flag 0x01 are decompressed
    /// using the encoding from the `Connect-Content-Encoding` header.
    pub fn decode_enveloped_bytes<T>(ctx: &ConnectContext, body: Bytes) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        // Parse envelope header: [flags:1][length:4][payload:length]
        if body.len() < 5 {
            return Err(ContextError::new(
                ctx.protocol,
                ConnectError::new(Code::InvalidArgument, "protocol error: incomplete envelope"),
            ));
        }

        let flags = body[0];
        let length = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;

        // Validate frame length
        let expected_len = 5 + length;
        if body.len() > expected_len {
            return Err(ContextError::new(
                ctx.protocol,
                ConnectError::new(
                    Code::InvalidArgument,
                    format!(
                        "frame has {} unexpected trailing bytes",
                        body.len() - expected_len
                    ),
                ),
            ));
        } else if body.len() < expected_len {
            return Err(ContextError::new(
                ctx.protocol,
                ConnectError::new(
                    Code::InvalidArgument,
                    format!(
                        "incomplete frame: expected {} bytes, got {}",
                        expected_len,
                        body.len()
                    ),
                ),
            ));
        }

        // Extract payload and process (validate flags + decompress)
        let raw_payload = body.slice(5..expected_len);
        let encoding = ctx
            .compression
            .envelope
            .map(|e| e.request)
            .unwrap_or(CompressionEncoding::Identity);

        let payload = process_envelope_payload(flags, raw_payload, encoding)
            .map_err(|e| ContextError::new(ctx.protocol, e))?
            .ok_or_else(|| {
                ContextError::new(
                    ctx.protocol,
                    ConnectError::new(Code::InvalidArgument, "unexpected EndStreamResponse in request"),
                )
            })?;

        Self::decode_message(ctx, &payload)
    }

    /// Helper: decode message based on protocol.
    fn decode_message<T>(ctx: &ConnectContext, bytes: &[u8]) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        if ctx.protocol.is_proto() {
            decode_proto(bytes).map_err(|e| ContextError::new(ctx.protocol, e))
        } else {
            decode_json(bytes).map_err(|e| ContextError::new(ctx.protocol, e))
        }
    }
}

/// Connect request wrapper for extracting messages from HTTP requests.
///
/// This type supports both single messages and streaming:
/// - `ConnectRequest<T>` - extracts a single message (for unary/server-streaming handlers)
/// - `ConnectRequest<Streaming<T>>` - extracts a message stream (for client-streaming/bidi handlers)
#[derive(Debug, Clone)]
pub struct ConnectRequest<T>(pub T);

/// A stream of messages from the client.
///
/// Used with `ConnectRequest<Streaming<T>>` for client-streaming and bidirectional streaming RPCs.
/// Similar to Tonic's `Streaming<T>` type.
///
/// # Example
///
/// ```ignore
/// async fn client_stream_handler(
///     req: ConnectRequest<Streaming<MyMessage>>,
/// ) -> Result<ConnectResponse<MyResponse>, ConnectError> {
///     let mut stream = req.0.into_stream();
///     while let Some(msg) = stream.next().await {
///         // process msg
///     }
///     Ok(ConnectResponse::new(MyResponse { ... }))
/// }
/// ```
pub struct Streaming<T> {
    inner: Pin<Box<dyn Stream<Item = Result<T, ConnectError>> + Send>>,
}

impl<T> Streaming<T> {
    /// Create a new Streaming from a boxed stream.
    pub fn new(stream: Pin<Box<dyn Stream<Item = Result<T, ConnectError>> + Send>>) -> Self {
        Self { inner: stream }
    }

    /// Convert into the underlying stream.
    pub fn into_stream(self) -> Pin<Box<dyn Stream<Item = Result<T, ConnectError>> + Send>> {
        self.inner
    }

    /// Create a Streaming from a tonic::Streaming.
    ///
    /// This is used internally by the TonicCompatibleBuilder to convert
    /// gRPC streaming requests into Connect streaming requests.
    #[cfg(feature = "tonic")]
    pub fn from_tonic(tonic_stream: tonic::Streaming<T>) -> Self
    where
        T: Send + 'static,
    {
        use futures::StreamExt;
        let mapped = tonic_stream.map(|result| result.map_err(ConnectError::from));
        Self {
            inner: Box::pin(mapped),
        }
    }
}

impl<T> Stream for Streaming<T> {
    type Item = Result<T, ConnectError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl<S, T> FromRequest<S> for ConnectRequest<T>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    type Rejection = ConnectError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match *req.method() {
            Method::POST => {
                // Get context (with fallback to default if layer is missing)
                let ctx = get_context_or_default(&req);

                // Dispatch based on protocol - no envelope for unary, envelope for streaming
                if ctx.protocol.needs_envelope() {
                    from_streaming_post_request(req, ctx).await
                } else {
                    from_unary_post_request(req).await
                }
            }
            Method::GET => from_get_request(req, state).await,
            _ => Err(ConnectError::new(
                Code::Unimplemented,
                "HTTP method not supported".to_string(),
            )),
        }
    }
}

/// Handle unary POST requests (application/json, application/proto).
///
/// Flow: read_body → decompress → check_size → decode
/// No envelope handling.
async fn from_unary_post_request<T>(req: Request) -> Result<ConnectRequest<T>, ConnectError>
where
    T: Message + DeserializeOwned + Default,
{
    RequestPipeline::decode::<T>(req)
        .await
        .map(ConnectRequest)
        .map_err(|e| e.into_connect_error())
}

/// Handle streaming-style POST requests used for unary (application/connect+json, application/connect+proto).
///
/// Flow: read_body → decode_enveloped_bytes (unwrap envelope → decompress if flag 0x01 → decode)
/// Handles per-envelope compression via Connect-Content-Encoding header.
async fn from_streaming_post_request<T>(
    req: Request,
    ctx: crate::context::ConnectContext,
) -> Result<ConnectRequest<T>, ConnectError>
where
    T: Message + DeserializeOwned + Default,
{
    // 1. Read body with size limit
    let max_size = ctx.limits.receive_max_bytes_or_max();
    let bytes = read_body(req.into_body(), max_size).await?;

    // 2. Decompress, check size, unwrap envelope, and decode
    RequestPipeline::decode_enveloped_bytes(&ctx, bytes)
        .map(ConnectRequest)
        .map_err(|e| e.into_connect_error())
}

/// Query parameters for GET unary requests.
///
/// Note: Validation of required parameters (encoding, message) and their values
/// is done in the layer via `validate_get_query_params()`. This struct uses
/// Option for all fields to handle parse errors gracefully.
#[derive(Deserialize, Default)]
struct GetRequestQuery {
    /// Connect protocol version (should be "v1" when present).
    /// Validation done in layer; kept here for secondary validation.
    #[serde(default)]
    connect: Option<String>,
    /// Message encoding - not used here since protocol is from Context.
    /// Protocol detection uses Context set by layer, not this field.
    #[serde(default)]
    #[allow(dead_code)]
    encoding: Option<String>,
    /// The message payload (required, but validated in layer).
    #[serde(default)]
    message: Option<String>,
    /// Whether the message is base64-encoded ("1" if true).
    #[serde(default)]
    base64: Option<String>,
    /// Compression algorithm used on the message (e.g., "gzip").
    #[serde(default)]
    compression: Option<String>,
}

async fn from_get_request<S, T>(req: Request, _state: &S) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    // Get context (with fallback to default if layer is missing)
    let ctx = get_context_or_default(&req);

    let query = req.uri().query().unwrap_or("");
    let params: GetRequestQuery = serde_qs::from_str(query)
        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;

    // Secondary connect version check (primary validation in layer)
    // This handles edge cases like connect being empty vs missing
    if let Some(ref connect) = params.connect
        && !connect.is_empty()
        && connect != "v1"
    {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!("connect must be \"v1\": got \"{}\"", connect),
        ));
    }

    // Get message content (layer validation ensures this is present)
    let message_str = params.message.unwrap_or_default();

    // 1. Decode base64 if specified (handle both padded and unpadded)
    let bytes = if params.base64.as_deref() == Some("1") {
        use base64::{
            Engine as _, alphabet,
            engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
        };
        // URL-safe base64 decoder that accepts both padded and unpadded input
        const URL_SAFE_INDIFFERENT: GeneralPurpose = GeneralPurpose::new(
            &alphabet::URL_SAFE,
            GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
        );
        URL_SAFE_INDIFFERENT
            .decode(&message_str)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        message_str.into_bytes()
    };

    // 2. Decompress if compression is specified
    let bytes = match params.compression.as_deref() {
        #[cfg(feature = "compression-gzip")]
        Some("gzip") => decompress_bytes(bytes.into(), CompressionEncoding::Gzip)?,
        Some("identity") | Some("") | None => bytes.into(),
        Some(other) => {
            // This should be caught by layer validation, but handle as fallback
            return Err(ConnectError::new(
                Code::Unimplemented,
                format!(
                    "unknown compression \"{}\": supported encodings are {}",
                    other,
                    connectrpc_axum_core::supported_encodings_str()
                ),
            ));
        }
    };

    // 3. Check size after decompression
    let bytes = read_frame_bytes(bytes, ctx.limits.receive_max_bytes_or_max())?;

    // 4. Decode based on protocol encoding
    let message = if ctx.protocol.is_proto() {
        decode_proto(&bytes)?
    } else {
        decode_json(&bytes)?
    };

    Ok(ConnectRequest(message))
}

/// `FromRequest` implementation for streaming requests using the unified `ConnectRequest<Streaming<T>>` pattern.
///
/// This enables handlers to use the same `ConnectRequest` wrapper for both unary and streaming:
/// - `ConnectRequest<T>` - single message (unary, server-streaming input)
/// - `ConnectRequest<Streaming<T>>` - message stream (client-streaming, bidi input)
impl<S, T> FromRequest<S> for ConnectRequest<Streaming<T>>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default + Send + 'static,
{
    type Rejection = ConnectError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        // Only POST is supported for streaming requests
        if *req.method() != Method::POST {
            return Err(ConnectError::new(
                Code::Unimplemented,
                "streaming requests only support POST method",
            ));
        }

        // Get context (with fallback to default if layer is missing)
        let ctx = get_context_or_default(&req);

        let use_proto = ctx.protocol.is_proto();
        // Get envelope compression settings (for streaming, this should be Some)
        let request_encoding = ctx
            .compression
            .envelope
            .map(|e| e.request)
            .unwrap_or(CompressionEncoding::Identity);
        let body = req.into_body();

        let stream = create_frame_stream::<T>(body, use_proto, ctx.limits, request_encoding);
        Ok(ConnectRequest(Streaming::new(Box::pin(stream))))
    }
}

/// Creates a stream that parses Connect frames from the request body.
///
/// Handles per-message compression: frames with flag 0x01 are decompressed
/// using the encoding from the `Connect-Content-Encoding` header.
fn create_frame_stream<T>(
    body: Body,
    use_proto: bool,
    limits: MessageLimits,
    request_encoding: CompressionEncoding,
) -> impl Stream<Item = Result<T, ConnectError>> + Send
where
    T: Message + DeserializeOwned + Default + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = BytesMut::new();
        let mut body = body;

        loop {
            // Try to parse a complete frame from the buffer
            while buffer.len() >= 5 {
                let flags = buffer[0];
                let length = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]) as usize;

                // Check message size limit BEFORE allocating memory
                if let Err(err) = limits.check_size(length) {
                    yield Err(ConnectError::new(Code::ResourceExhausted, err));
                    return;
                }

                // Check if we have the complete frame
                if buffer.len() < 5 + length {
                    // Pre-allocate space for the frame to reduce reallocations
                    buffer.reserve(5 + length - buffer.len());
                    break; // Need more data
                }

                // Extract payload
                let raw_payload = buffer.split_to(5 + length).split_off(5);

                // Process envelope: validate flags and decompress if needed
                let payload = match process_envelope_payload(flags, raw_payload.freeze(), request_encoding) {
                    Ok(Some(payload)) => payload,
                    Ok(None) => {
                        // EndStream frame - client stream is done
                        return;
                    }
                    Err(err) => {
                        yield Err(err);
                        return;
                    }
                };

                // Decode the message using pipeline primitives
                let message = if use_proto {
                    decode_proto(&payload)
                } else {
                    decode_json(&payload)
                };

                yield message;
            }

            // Read more data from body
            match body.frame().await {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        buffer.extend_from_slice(data);
                    }
                }
                Some(Err(err)) => {
                    yield Err(ConnectError::new(
                        Code::Unknown,
                        format!("read enveloped message: {err}"),
                    ));
                    return;
                }
                None => {
                    // Body exhausted
                    if !buffer.is_empty() {
                        yield Err(ConnectError::new(
                            Code::InvalidArgument,
                            format!("protocol error: incomplete envelope: {} trailing bytes", buffer.len()),
                        ));
                    }
                    return;
                }
            }
        }
    }
}
