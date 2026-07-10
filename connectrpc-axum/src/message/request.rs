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
/// Returns `ResourceExhausted` if the body exceeds `max_size`, or an
/// `Internal` error for other body read failures.
pub async fn read_body(body: Body, max_size: usize) -> Result<Bytes, ConnectError> {
    axum::body::to_bytes(body, max_size).await.map_err(|e| {
        // Distinguish "body too large" from other transport/body errors by
        // inspecting the error source chain.
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&e);
        while let Some(err) = source {
            if err.is::<http_body_util::LengthLimitError>() {
                return ConnectError::new(
                    Code::ResourceExhausted,
                    format!("request body exceeds maximum allowed size of {max_size} bytes"),
                );
            }
            source = err.source();
        }
        ConnectError::new(Code::Internal, format!("failed to read request body: {e}"))
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

/// Decompress bytes based on compression encoding, bounding the output size.
///
/// Returns the original bytes unchanged (zero-copy) if encoding is `Identity`.
///
/// `max_output` caps the decompressed size to defend against decompression
/// bombs: decoding aborts as soon as the output would exceed it, so peak memory
/// stays bounded regardless of the compressed input's expansion ratio. Pass
/// `usize::MAX` (e.g. via [`MessageLimits::receive_max_bytes_or_max`]) for no
/// limit.
///
/// Returns `ResourceExhausted` if the decompressed payload exceeds `max_output`,
/// or `InvalidArgument` if decompression otherwise fails.
pub fn decompress_bytes(
    bytes: Bytes,
    encoding: CompressionEncoding,
    max_output: usize,
) -> Result<Bytes, ConnectError> {
    let Some(codec) = encoding.codec() else {
        // Identity: no decompression, but still enforce the output cap so the
        // documented `max_output` contract holds regardless of encoding.
        return enforce_max_output(bytes, max_output);
    };

    codec
        .decompress_limited(&bytes, max_output)
        .map_err(|e| match e {
            connectrpc_axum_core::DecompressError::TooLarge { limit } => ConnectError::new(
                Code::ResourceExhausted,
                format!("decompressed message size exceeds maximum allowed size of {limit} bytes"),
            ),
            e => ConnectError::new(Code::InvalidArgument, format!("decompression failed: {e}")),
        })
}

/// Return `bytes` unchanged unless its length exceeds `max_output`, in which case
/// a `ResourceExhausted` error is returned. Used for the identity/uncompressed
/// paths so they honor the same size cap as the decompressing paths.
fn enforce_max_output(bytes: Bytes, max_output: usize) -> Result<Bytes, ConnectError> {
    if bytes.len() > max_output {
        return Err(ConnectError::new(
            Code::ResourceExhausted,
            format!("message size exceeds maximum allowed size of {max_output} bytes"),
        ));
    }
    Ok(bytes)
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
pub use connectrpc_axum_core::envelope_flags;

/// Processed envelope payload, classified by frame type.
pub use connectrpc_axum_core::EnvelopePayload;

/// Convert an [`EnvelopeError`] into a [`ConnectError`] with the appropriate code.
fn envelope_error_to_connect(err: connectrpc_axum_core::EnvelopeError) -> ConnectError {
    use connectrpc_axum_core::EnvelopeError;
    let code = match &err {
        EnvelopeError::MessageTooLarge { .. } => Code::ResourceExhausted,
        // connect-go reports unknown envelope flag bits as Internal
        // ("protocol error: invalid envelope flags %d").
        EnvelopeError::Compression(_)
        | EnvelopeError::PayloadTooLarge { .. }
        | EnvelopeError::MissingCompression
        | EnvelopeError::InvalidFlags(_) => Code::Internal,
        EnvelopeError::IncompleteHeader { .. } | EnvelopeError::Decompression(_) => {
            Code::InvalidArgument
        }
        // EnvelopeError is #[non_exhaustive]; future variants are protocol
        // errors, which connect-go reports as Internal.
        _ => Code::Internal,
    };
    ConnectError::new(code, err.to_string())
}

/// Process envelope payload based on flags, with optional decompression.
///
/// Delegates to [`connectrpc_axum_core::process_envelope_payload`], converting
/// errors into [`ConnectError`]s with appropriate codes.
///
/// Flags are a bitfield per the Connect spec, so they are matched with bitwise
/// masks rather than exact equality: a compressed end-stream frame is `0x03`
/// (`COMPRESSED | END_STREAM`), not a distinct value.
///
/// # Returns
/// - `Ok(EnvelopePayload::Message(payload))` for message frames (END_STREAM bit clear)
/// - `Ok(EnvelopePayload::EndStream(payload))` for end-stream frames (END_STREAM bit set)
/// - `Err` for flags with unknown bits set, decompression failures, or payloads
///   exceeding `max_output` (`ResourceExhausted`)
///
/// # Arguments
/// - `flags`: The envelope flags byte
/// - `payload`: The raw payload bytes from the envelope
/// - `encoding`: Compression encoding to use for decompression (from `Connect-Content-Encoding`)
/// - `max_output`: Upper bound on the decompressed payload size (decompression-bomb guard).
///   Pass `usize::MAX` for no limit.
pub fn process_envelope_payload(
    flags: u8,
    payload: Bytes,
    encoding: CompressionEncoding,
    max_output: usize,
) -> Result<EnvelopePayload, ConnectError> {
    let max_output = (max_output != usize::MAX).then_some(max_output);
    connectrpc_axum_core::process_envelope_payload(flags, payload, encoding, max_output)
        .map_err(envelope_error_to_connect)
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
            .map_err(|e| ContextError::new(ctx.protocol, e, ctx.limits.get_send_max_bytes()))?;

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
                ctx.limits.get_send_max_bytes(),
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
                ctx.limits.get_send_max_bytes(),
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
                ctx.limits.get_send_max_bytes(),
            ));
        }

        // Extract payload and process (validate flags + decompress)
        let raw_payload = body.slice(5..expected_len);
        let encoding = ctx
            .compression
            .envelope
            .map(|e| e.request)
            .unwrap_or(CompressionEncoding::Identity);

        let payload = match process_envelope_payload(
            flags,
            raw_payload,
            encoding,
            ctx.limits.receive_max_bytes_or_max(),
        )
        .map_err(|e| ContextError::new(ctx.protocol, e, ctx.limits.get_send_max_bytes()))?
        {
            EnvelopePayload::Message(payload) => payload,
            EnvelopePayload::EndStream(_) => {
                // connect-go reads the single request message through its
                // streaming machinery, where an end-stream frame surfaces as
                // EOF, i.e. a cardinality violation (zero messages), which the
                // gRPC status-code spec maps to Unimplemented.
                return Err(ContextError::new(
                    ctx.protocol,
                    ConnectError::new(Code::Unimplemented, "unary request has zero messages"),
                    ctx.limits.get_send_max_bytes(),
                ));
            }
        };

        Self::decode_message(ctx, &payload)
    }

    /// Helper: decode message based on protocol.
    fn decode_message<T>(ctx: &ConnectContext, bytes: &[u8]) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        if ctx.protocol.is_proto() {
            decode_proto(bytes)
                .map_err(|e| ContextError::new(ctx.protocol, e, ctx.limits.get_send_max_bytes()))
        } else {
            decode_json(bytes)
                .map_err(|e| ContextError::new(ctx.protocol, e, ctx.limits.get_send_max_bytes()))
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
    let max_size = ctx.limits.receive_max_bytes_or_max().saturating_add(5);
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

    // 2. Decompress if compression is specified (bounded to guard against bombs).
    // Uses the same codec resolution as the POST path so every enabled encoding works.
    let compression = params.compression.as_deref();
    let bytes = match CompressionEncoding::from_header(compression) {
        Some(encoding) => decompress_bytes(
            bytes.into(),
            encoding,
            ctx.limits.receive_max_bytes_or_max(),
        )?,
        None => {
            // This should be caught by layer validation, but handle as fallback
            return Err(ConnectError::new(
                Code::Unimplemented,
                format!(
                    "unknown compression \"{}\": supported encodings are {}",
                    compression.unwrap_or(""),
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
                let Some(frame_len) = 5usize.checked_add(length) else {
                    yield Err(ConnectError::new(
                        Code::ResourceExhausted,
                        "message size exceeds platform address space",
                    ));
                    return;
                };

                // Check message size limit BEFORE allocating memory
                if let Err(err) = limits.check_size_connect(length) {
                    yield Err(err);
                    return;
                }

                // Check if we have the complete frame
                if buffer.len() < frame_len {
                    break; // Need more data
                }

                // Extract payload
                let raw_payload = buffer.split_to(frame_len).split_off(5);

                // Process envelope: validate flags and decompress if needed.
                // Bound decompression output to the receive limit (bomb guard).
                let payload = match process_envelope_payload(
                    flags,
                    raw_payload.freeze(),
                    request_encoding,
                    limits.receive_max_bytes_or_max(),
                ) {
                    Ok(EnvelopePayload::Message(payload)) => payload,
                    Ok(EnvelopePayload::EndStream(payload)) => {
                        // EndStream frame - the client stream is done, but only
                        // gracefully if the frame passes the same validation
                        // connect-go applies (nothing follows it, payload is a
                        // well-formed end-stream message).
                        if let Err(err) =
                            validate_client_end_stream(&buffer, &mut body, &payload).await
                        {
                            yield Err(err);
                        }
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

/// Wire shape of the Connect end-stream message, mirroring connect-go's
/// `connectEndStreamMessage`. Used only to validate client-sent end-stream
/// frames; the contents are discarded.
#[derive(Deserialize)]
#[allow(dead_code)]
struct ClientEndStreamMessage {
    #[serde(default)]
    error: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    metadata: Option<std::collections::HashMap<String, Vec<String>>>,
}

/// Validate a client-sent end-stream frame the way connect-go does.
///
/// connect-go treats a client-sent end-stream frame as a graceful end of the
/// request stream, but only after verifying that nothing follows the frame and
/// that its payload is a well-formed end-stream message; either violation is
/// an `Internal` protocol error. Checked in that order, and with the same
/// messages, as connect-go (which reuses its client-side "corrupt response"
/// wording on the server).
async fn validate_client_end_stream(
    buffered: &BytesMut,
    body: &mut Body,
    payload: &[u8],
) -> Result<(), ConnectError> {
    // Drain the rest of the stream to ensure there is no extra data.
    let mut extra = buffered.len();
    loop {
        match body.frame().await {
            Some(Ok(frame)) => {
                if let Some(data) = frame.data_ref() {
                    extra += data.len();
                }
            }
            Some(Err(err)) => {
                return Err(ConnectError::new(
                    Code::Internal,
                    format!("corrupt response: I/O error after end-stream message: {err}"),
                ));
            }
            None => break,
        }
    }
    if extra > 0 {
        return Err(ConnectError::new(
            Code::Internal,
            format!("corrupt response: {extra} extra bytes after end of stream"),
        ));
    }

    // The end-stream payload is always JSON, regardless of the negotiated
    // codec. `Option` mirrors Go's json.Unmarshal accepting a literal `null`.
    serde_json::from_slice::<Option<ClientEndStreamMessage>>(payload)
        .map(drop)
        .map_err(|err| {
            ConnectError::new(
                Code::Internal,
                format!("unmarshal end stream message: {err}"),
            )
        })
}

#[cfg(test)]
mod max_output_tests {
    //! The `max_output` cap must hold on the identity/uncompressed paths too,
    //! not only when decompression runs (issue #53 follow-up).
    use super::*;

    #[test]
    fn decompress_bytes_identity_enforces_max_output() {
        let data = Bytes::from(vec![0u8; 100]);
        // Within limit: returned unchanged.
        let out = decompress_bytes(data.clone(), CompressionEncoding::Identity, 100).unwrap();
        assert_eq!(out.len(), 100);
        // Over limit: rejected even though no decompression happens.
        let err = decompress_bytes(data, CompressionEncoding::Identity, 99).unwrap_err();
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn decompress_bytes_identity_unbounded_with_usize_max() {
        let data = Bytes::from(vec![0u8; 4096]);
        let out = decompress_bytes(data, CompressionEncoding::Identity, usize::MAX).unwrap();
        assert_eq!(out.len(), 4096);
    }

    #[test]
    fn process_envelope_payload_uncompressed_enforces_max_output() {
        let payload = Bytes::from(vec![0u8; 100]);
        // Uncompressed frame within limit passes.
        let out = process_envelope_payload(
            envelope_flags::MESSAGE,
            payload.clone(),
            CompressionEncoding::Identity,
            100,
        )
        .unwrap();
        assert_eq!(out, EnvelopePayload::Message(payload.clone()));
        // Uncompressed frame over limit is rejected (was previously unchecked).
        let err = process_envelope_payload(
            envelope_flags::MESSAGE,
            payload,
            CompressionEncoding::Identity,
            99,
        )
        .unwrap_err();
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn process_envelope_payload_end_stream_yields_payload() {
        // EndStream frames yield their payload for the caller to parse.
        let out = process_envelope_payload(
            envelope_flags::END_STREAM,
            Bytes::from_static(b"{}"),
            CompressionEncoding::Identity,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(out, EnvelopePayload::EndStream(Bytes::from_static(b"{}")));
    }

    #[test]
    fn process_envelope_payload_compressed_without_negotiation_is_error() {
        // A COMPRESSED frame with Identity negotiated is a protocol error
        // (connect-go: "sent compressed message without compression support"),
        // not a passthrough that fails later during decode.
        let flags = envelope_flags::COMPRESSED;
        let err = process_envelope_payload(
            flags,
            Bytes::from_static(b"data"),
            CompressionEncoding::Identity,
            usize::MAX,
        )
        .unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert!(
            err.message()
                .unwrap()
                .contains("sent compressed message without compression support")
        );
    }

    #[test]
    fn process_envelope_payload_unknown_flag_bits_rejected() {
        // Bits outside COMPRESSED | END_STREAM are a protocol error, reported
        // as Internal like connect-go's "protocol error: invalid envelope
        // flags %d".
        let err = process_envelope_payload(
            0x80,
            Bytes::from_static(b"x"),
            CompressionEncoding::Identity,
            usize::MAX,
        )
        .unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert_eq!(
            err.message().unwrap(),
            "protocol error: invalid envelope flags 128"
        );
    }
}

#[cfg(test)]
mod client_end_stream_tests {
    //! A client-sent end-stream frame ends the request stream gracefully, but
    //! only after the validation connect-go applies: nothing may follow the
    //! frame and its payload must be a well-formed end-stream message.
    //! Expectations below were verified against connect-go's server behavior.
    use super::*;
    use futures::StreamExt;

    fn frame(flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(5 + payload.len());
        buf.push(flags);
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(payload);
        buf
    }

    fn message_frame() -> Vec<u8> {
        frame(
            envelope_flags::MESSAGE,
            &pbjson_types::Empty::default().encode_to_vec(),
        )
    }

    async fn collect(body: Vec<u8>) -> Vec<Result<pbjson_types::Empty, ConnectError>> {
        create_frame_stream::<pbjson_types::Empty>(
            Body::from(body),
            true,
            MessageLimits::new(),
            CompressionEncoding::Identity,
        )
        .collect()
        .await
    }

    #[tokio::test]
    async fn end_stream_frame_ends_stream_gracefully() {
        let items = collect(frame(envelope_flags::END_STREAM, b"{}")).await;
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn message_then_end_stream_frame_is_graceful() {
        let mut body = message_frame();
        body.extend_from_slice(&frame(envelope_flags::END_STREAM, b"{}"));
        let items = collect(body).await;
        assert_eq!(items.len(), 1);
        assert!(items[0].is_ok());
    }

    #[tokio::test]
    async fn end_stream_frame_with_metadata_is_graceful() {
        let items = collect(frame(
            envelope_flags::END_STREAM,
            br#"{"metadata":{"x-extra":["1"]}}"#,
        ))
        .await;
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn end_stream_frame_with_invalid_json_is_internal() {
        let items = collect(frame(envelope_flags::END_STREAM, b"notjson")).await;
        assert_eq!(items.len(), 1);
        let err = items[0].as_ref().unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert!(
            err.message()
                .unwrap()
                .starts_with("unmarshal end stream message:")
        );
    }

    #[tokio::test]
    async fn bytes_after_end_stream_frame_are_internal() {
        let mut body = frame(envelope_flags::END_STREAM, b"{}");
        body.extend_from_slice(&message_frame());
        let extra = message_frame().len();
        let items = collect(body).await;
        assert_eq!(items.len(), 1);
        let err = items[0].as_ref().unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert_eq!(
            err.message().unwrap(),
            format!("corrupt response: {extra} extra bytes after end of stream")
        );
    }

    // The extra-bytes check runs before payload validation, matching connect-go.
    #[tokio::test]
    async fn extra_bytes_reported_before_invalid_json() {
        let mut body = frame(envelope_flags::END_STREAM, b"notjson");
        body.extend_from_slice(b"junk");
        let items = collect(body).await;
        assert_eq!(items.len(), 1);
        let err = items[0].as_ref().unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert!(
            err.message()
                .unwrap()
                .starts_with("corrupt response: 4 extra bytes")
        );
    }

    #[test]
    fn enveloped_unary_end_stream_frame_is_cardinality_violation() {
        // connect-go: an end-stream frame instead of the single request
        // message reads as EOF → "unary request has zero messages"
        // (Unimplemented).
        let ctx = ConnectContext {
            protocol: crate::context::RequestProtocol::ConnectStreamProto,
            ..Default::default()
        };
        let err = RequestPipeline::decode_enveloped_bytes::<pbjson_types::Empty>(
            &ctx,
            Bytes::from(frame(envelope_flags::END_STREAM, b"{}")),
        )
        .unwrap_err()
        .into_connect_error();
        assert_eq!(err.code(), Code::Unimplemented);
        assert_eq!(err.message().unwrap(), "unary request has zero messages");
    }
}

#[cfg(test)]
mod incremental_frame_read_tests {
    use super::*;
    use futures::{StreamExt, stream};
    use std::{convert::Infallible, time::Duration};

    #[tokio::test]
    async fn maximum_declared_frame_length_waits_for_body_without_preallocation() {
        let chunks = stream::once(async {
            Ok::<_, Infallible>(Bytes::from_static(&[0, 0xff, 0xff, 0xff, 0xff]))
        })
        .chain(stream::pending());
        let body = Body::from_stream(chunks);
        let mut messages = Box::pin(create_frame_stream::<pbjson_types::Empty>(
            body,
            true,
            MessageLimits::new(),
            CompressionEncoding::Identity,
        ));

        assert!(
            tokio::time::timeout(Duration::from_millis(25), messages.next())
                .await
                .is_err()
        );
    }
}

#[cfg(all(test, feature = "compression-gzip-stream"))]
mod decompression_bomb_tests {
    //! Regression tests for the streaming decompression-bomb guard (issue #53).
    //!
    //! A small compressed frame must not be able to expand past `receive_max_bytes`
    //! during decompression on any streaming request path.
    use super::*;
    use crate::context::{CompressionContext, EnvelopeCompression, RequestProtocol};
    use futures::StreamExt;

    /// gzip-compress `decompressed_len` zero bytes (compresses to a tiny frame).
    fn gzip_bomb(decompressed_len: usize) -> Bytes {
        CompressionEncoding::Gzip
            .codec()
            .unwrap()
            .compress(&vec![0u8; decompressed_len])
            .unwrap()
    }

    fn compressed_frame(payload: &Bytes) -> Bytes {
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(envelope_flags::COMPRESSED);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        Bytes::from(frame)
    }

    // process_envelope_payload is the decompression chokepoint shared by both the
    // single-envelope and streaming-frame request paths.
    #[test]
    fn process_envelope_payload_rejects_bomb() {
        let bomb = gzip_bomb(1024 * 1024); // 1 MiB -> ~KiB compressed
        let err = process_envelope_payload(
            envelope_flags::COMPRESSED,
            bomb,
            CompressionEncoding::Gzip,
            64 * 1024,
        )
        .unwrap_err();
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn process_envelope_payload_allows_within_limit() {
        let payload = vec![1u8; 1000];
        let compressed = CompressionEncoding::Gzip
            .codec()
            .unwrap()
            .compress(&payload)
            .unwrap();
        let out = process_envelope_payload(
            envelope_flags::COMPRESSED,
            compressed,
            CompressionEncoding::Gzip,
            64 * 1024,
        )
        .unwrap();
        assert_eq!(out, EnvelopePayload::Message(Bytes::from(payload)));
    }

    // End-to-end: the frame's *compressed* length passes the size check, but
    // decompression must still be bounded and abort with ResourceExhausted.
    #[tokio::test]
    async fn create_frame_stream_bounds_decompression() {
        let bomb = gzip_bomb(2 * 1024 * 1024); // decompresses to 2 MiB
        let limit = 100 * 1024;
        assert!(
            bomb.len() < limit,
            "compressed frame ({} bytes) must pass the compressed-size check",
            bomb.len()
        );

        let body = Body::from(compressed_frame(&bomb));
        let limits = MessageLimits::new().receive_max_bytes(limit);
        let mut stream = Box::pin(create_frame_stream::<pbjson_types::Empty>(
            body,
            true,
            limits,
            CompressionEncoding::Gzip,
        ));

        let first = stream.next().await.expect("stream should yield an item");
        assert_eq!(first.unwrap_err().code(), Code::ResourceExhausted);
    }

    // Single-envelope streaming POST path (decode_enveloped_bytes).
    #[test]
    fn decode_enveloped_bytes_bounds_decompression() {
        let frame = compressed_frame(&gzip_bomb(1024 * 1024));
        let ctx = ConnectContext {
            protocol: RequestProtocol::ConnectStreamProto,
            compression: CompressionContext {
                envelope: Some(EnvelopeCompression {
                    request: CompressionEncoding::Gzip,
                    response: CompressionEncoding::Identity,
                }),
                ..Default::default()
            },
            limits: MessageLimits::new().receive_max_bytes(64 * 1024),
            ..Default::default()
        };

        let err = RequestPipeline::decode_enveloped_bytes::<pbjson_types::Empty>(&ctx, frame)
            .unwrap_err()
            .into_connect_error();
        assert_eq!(err.code(), Code::ResourceExhausted);
    }
}

#[cfg(test)]
mod read_body_tests {
    //! `read_body` must distinguish "body too large" (`ResourceExhausted`)
    //! from other body read failures (`Internal`).
    use super::*;

    #[tokio::test]
    async fn read_body_over_limit_is_resource_exhausted() {
        let body = Body::from(vec![0u8; 1024]);
        let err = read_body(body, 100).await.unwrap_err();
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn read_body_within_limit_ok() {
        let body = Body::from(vec![0u8; 100]);
        let bytes = read_body(body, 100).await.unwrap();
        assert_eq!(bytes.len(), 100);
    }

    #[tokio::test]
    async fn read_body_stream_error_is_internal() {
        let stream = futures::stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b"data")),
            Err(std::io::Error::other("connection reset")),
        ]);
        let body = Body::from_stream(stream);
        let err = read_body(body, usize::MAX).await.unwrap_err();
        assert_eq!(err.code(), Code::Internal);
    }
}

#[cfg(all(test, feature = "compression-gzip-stream"))]
mod get_request_compression_tests {
    //! GET `compression=` handling must resolve codecs the same way the POST
    //! path does, so every enabled encoding works (not just gzip).
    use super::*;
    use axum::http::Request as HttpRequest;
    use base64::Engine as _;

    async fn decode_get_request(
        query: &str,
    ) -> Result<ConnectRequest<pbjson_types::Empty>, ConnectError> {
        let req = HttpRequest::builder()
            .method(Method::GET)
            .uri(format!("/svc/Method?{query}"))
            .body(Body::empty())
            .unwrap();
        ConnectRequest::<pbjson_types::Empty>::from_request(req, &()).await
    }

    #[tokio::test]
    async fn get_request_gzip_compression_decodes() {
        let payload = pbjson_types::Empty::default().encode_to_vec();
        let compressed = CompressionEncoding::Gzip
            .codec()
            .unwrap()
            .compress(&payload)
            .unwrap();
        let message = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&compressed);
        let query =
            format!("connect=v1&encoding=proto&message={message}&base64=1&compression=gzip");
        decode_get_request(&query).await.unwrap();
    }

    #[cfg(feature = "compression-br-stream")]
    #[tokio::test]
    async fn get_request_br_compression_decodes() {
        let payload = pbjson_types::Empty::default().encode_to_vec();
        let compressed = CompressionEncoding::Brotli
            .codec()
            .unwrap()
            .compress(&payload)
            .unwrap();
        let message = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&compressed);
        let query = format!("connect=v1&encoding=proto&message={message}&base64=1&compression=br");
        decode_get_request(&query).await.unwrap();
    }

    #[tokio::test]
    async fn get_request_unknown_compression_is_unimplemented() {
        let err = decode_get_request("connect=v1&encoding=proto&message=&compression=lz4")
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::Unimplemented);
        assert!(err.message().unwrap().contains("supported encodings are"));
    }

    #[tokio::test]
    async fn get_request_identity_compression_decodes() {
        decode_get_request("connect=v1&encoding=proto&message=&compression=identity")
            .await
            .unwrap();
    }
}
