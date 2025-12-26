//! Extractor for Connect requests.
use crate::error::Code;
use crate::error::ConnectError;
use crate::limits::MessageLimits;
use crate::protocol::RequestProtocol;
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
            Method::POST => from_post_request(req, state).await,
            Method::GET => from_get_request(req, state).await,
            _ => Err(ConnectError::new(
                Code::Unimplemented,
                "HTTP method not supported".to_string(),
            )),
        }
    }
}

async fn from_post_request<S, T>(
    req: Request,
    _state: &S,
) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    // Protocol is detected by ConnectLayer middleware and stored in extensions
    let protocol = req
        .extensions()
        .get::<RequestProtocol>()
        .copied()
        .unwrap_or_default();

    // Get message limits from extensions (set by ConnectLayer)
    let limits = req
        .extensions()
        .get::<MessageLimits>()
        .copied()
        .unwrap_or_default();

    let mut bytes = Bytes::from_request(req, _state)
        .await
        .map_err(|err| ConnectError::new(Code::Internal, err.to_string()))?;

    // Check body size against limits
    limits
        .check_size(bytes.len())
        .map_err(|err| ConnectError::new(Code::ResourceExhausted, err))?;

    // For Connect streaming, unwrap the 5-byte frame envelope
    // Frame format: [flags:1][length:4][payload:length]
    // Note: gRPC requests are handled by Tonic via ContentTypeSwitch
    let needs_frame_unwrap = protocol.needs_envelope();

    if needs_frame_unwrap && bytes.len() >= 5 {
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

        // Extract the actual payload (unary request must have exactly one frame)
        if bytes.len() > 5 + length {
            return Err(ConnectError::new(
                Code::InvalidArgument,
                format!(
                    "frame has {} unexpected trailing bytes",
                    bytes.len() - 5 - length
                ),
            ));
        } else if bytes.len() < 5 + length {
            return Err(ConnectError::new(
                Code::InvalidArgument,
                format!(
                    "incomplete frame: expected {} bytes, got {}",
                    5 + length,
                    bytes.len()
                ),
            ));
        }
        bytes = bytes.slice(5..5 + length);
    } else if needs_frame_unwrap {
        // Frame expected but body is too short - this is an error
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "protocol error: incomplete envelope",
        ));
    }

    // Decode based on protocol encoding
    if protocol.is_proto() {
        let message = T::decode(bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;
        Ok(ConnectRequest(message))
    } else {
        let message: T = serde_json::from_slice(&bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;
        Ok(ConnectRequest(message))
    }
}

#[derive(Deserialize)]
struct GetRequestQuery {
    connect: String,
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
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::URL_SAFE
            .decode(&params.message)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        params.message.into_bytes()
    };

    let message = if params.encoding == "proto" {
        T::decode(bytes.as_slice())
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else if params.encoding == "json" {
        serde_json::from_slice(&bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported encoding",
        ));
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

        // Protocol is detected by ConnectLayer middleware and stored in extensions
        let protocol = req
            .extensions()
            .get::<RequestProtocol>()
            .copied()
            .unwrap_or_default();

        // Get message limits from extensions (set by ConnectLayer)
        let limits = req
            .extensions()
            .get::<MessageLimits>()
            .copied()
            .unwrap_or_default();

        let use_proto = protocol.is_proto();
        let body = req.into_body();

        let stream = create_frame_stream::<T>(body, use_proto, limits);
        Ok(ConnectStreamingRequest {
            stream: Box::pin(stream),
        })
    }
}

/// Creates a stream that parses Connect frames from the request body.
fn create_frame_stream<T>(
    body: Body,
    use_proto: bool,
    limits: MessageLimits,
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
                if flags == 0x02 {
                    // Client sent EndStream, we're done
                    return;
                }

                // Regular message frame must have flags = 0x00
                if flags != 0x00 {
                    yield Err(ConnectError::new(
                        Code::InvalidArgument,
                        format!("invalid Connect frame flags: 0x{:02x}", flags),
                    ));
                    return;
                }

                // Extract payload
                let payload = buffer.split_to(5 + length).split_off(5);

                // Decode the message
                let message = if use_proto {
                    T::decode(payload.freeze())
                        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))
                } else {
                    serde_json::from_slice(&payload)
                        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))
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
