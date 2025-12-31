//! Extractor for Connect requests.
use crate::context::{CompressionEncoding, Context, MessageLimits};
use crate::error::{Code, ConnectError};
use crate::pipeline::{
    decode_json, decode_proto, decompress_bytes, envelope_flags, read_body, unwrap_envelope,
    RequestPipeline,
};
use axum::{
    body::Body,
    extract::{FromRequest, Request},
    http::Method,
};
use bytes::BytesMut;
use futures::Stream;
use http_body_util::BodyExt;
use prost::Message;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::pin::Pin;

#[derive(Debug, Clone)]
pub struct ConnectRequest<T>(pub T);

impl<S, T> FromRequest<S> for ConnectRequest<T>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    type Rejection = ConnectError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match *req.method() {
            Method::POST => {
                // Get context to determine protocol type
                let ctx = req
                    .extensions()
                    .get::<Context>()
                    .cloned()
                    .ok_or_else(|| ConnectError::new(Code::Internal, "missing pipeline context"))?;

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
/// Flow: read_body → decompress → check_size → unwrap_envelope → decode
/// Has envelope handling.
async fn from_streaming_post_request<T>(
    req: Request,
    ctx: Context,
) -> Result<ConnectRequest<T>, ConnectError>
where
    T: Message + DeserializeOwned + Default,
{
    // 1. Read body with size limit
    let max_size = ctx.limits.max_message_size().unwrap_or(usize::MAX);
    let bytes = read_body(req.into_body(), max_size).await?;

    // 2. Decompress if needed
    let bytes = decompress_bytes(bytes, ctx.compression.request_encoding)?;

    // 3. Check size after decompression
    ctx.limits
        .check_size(bytes.len())
        .map_err(|e| ConnectError::new(Code::ResourceExhausted, e))?;

    // 4. Unwrap envelope and decode
    let payload = unwrap_envelope(&bytes)?;

    // 5. Decode based on protocol encoding
    if ctx.protocol.is_proto() {
        decode_proto(&payload).map(ConnectRequest)
    } else {
        decode_json(&payload).map(ConnectRequest)
    }
}

#[derive(Deserialize)]
struct GetRequestQuery {
    connect: String,
    #[allow(dead_code)] // Protocol detection uses Context from layer, not this field
    encoding: String,
    message: String,
    base64: Option<String>,
    #[allow(dead_code)] // Not used yet, but part of the spec
    compression: Option<String>,
}

async fn from_get_request<S, T>(req: Request, _state: &S) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    // Get protocol from Context (set by ConnectLayer via detect_protocol)
    let ctx = req
        .extensions()
        .get::<Context>()
        .cloned()
        .ok_or_else(|| ConnectError::new(Code::Internal, "missing pipeline context"))?;

    let query = req.uri().query().unwrap_or("");
    let params: GetRequestQuery = serde_qs::from_str(query)
        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;

    if params.connect != "v1" {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported connect version",
        ));
    }

    let bytes = if params.base64.as_deref() == Some("1") {
        use base64::{engine::general_purpose, Engine as _};
        general_purpose::URL_SAFE
            .decode(&params.message)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        params.message.into_bytes()
    };

    // Use protocol from Context instead of parsing encoding from query params
    let message = if ctx.protocol.is_proto() {
        decode_proto(&bytes)?
    } else {
        decode_json(&bytes)?
    };

    Ok(ConnectRequest(message))
}

/// Streaming request extractor for client streaming and bidi streaming RPCs.
///
/// Parses multiple frames from the request body into a `Stream`.
/// Each frame follows the Connect protocol format: `[flags:1][length:4][payload]`
///
/// This extractor is designed for generated code and does not support
/// additional Axum extractors like `ConnectRequest` does.
pub struct ConnectStreamingRequest<T> {
    /// Stream of decoded messages from the request body.
    pub stream: Pin<Box<dyn Stream<Item = Result<T, ConnectError>> + Send>>,
}

impl<S, T> FromRequest<S> for ConnectStreamingRequest<T>
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

        // Get pipeline context from extensions (injected by ConnectLayer)
        let ctx = req
            .extensions()
            .get::<Context>()
            .cloned()
            .ok_or_else(|| ConnectError::new(Code::Internal, "missing pipeline context"))?;

        let use_proto = ctx.protocol.is_proto();
        let request_encoding = ctx.compression.request_encoding;
        let body = req.into_body();

        let stream = create_frame_stream::<T>(body, use_proto, ctx.limits, request_encoding);
        Ok(ConnectStreamingRequest {
            stream: Box::pin(stream),
        })
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
                    break; // Need more data
                }

                // EndStream frame (flags = 0x02) signals end of client stream
                if flags == envelope_flags::END_STREAM {
                    // Client sent EndStream, we're done
                    return;
                }

                // Check for valid message flags (0x00 = uncompressed, 0x01 = compressed)
                let is_compressed = flags == envelope_flags::COMPRESSED;
                if flags != envelope_flags::MESSAGE && !is_compressed {
                    yield Err(ConnectError::new(
                        Code::InvalidArgument,
                        format!("invalid Connect frame flags: 0x{:02x}", flags),
                    ));
                    return;
                }

                // Extract payload
                let payload = buffer.split_to(5 + length).split_off(5);

                // Decompress if needed
                let payload = if is_compressed {
                    match decompress_bytes(payload.freeze(), request_encoding) {
                        Ok(decompressed) => {
                            // Check size after decompression
                            if let Err(err) = limits.check_size(decompressed.len()) {
                                yield Err(ConnectError::new(Code::ResourceExhausted, err));
                                return;
                            }
                            decompressed.into()
                        }
                        Err(err) => {
                            yield Err(err);
                            return;
                        }
                    }
                } else {
                    payload
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
                    yield Err(ConnectError::new(Code::Internal, err.to_string()));
                    return;
                }
                None => {
                    // Body exhausted
                    if !buffer.is_empty() {
                        yield Err(ConnectError::new(
                            Code::InvalidArgument,
                            format!("incomplete frame: {} trailing bytes", buffer.len()),
                        ));
                    }
                    return;
                }
            }
        }
    }
}
