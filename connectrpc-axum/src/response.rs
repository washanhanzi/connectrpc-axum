//! Response types for Connect.
use crate::error::ConnectError;
use crate::request::{ContentFormat, get_request_format};
use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::Stream;
use prost::Message;
use serde::Serialize;

const APPLICATION_JSON: &str = "application/json";
const APPLICATION_CONNECT_JSON: &str = "application/connect+json";
const APPLICATION_CONNECT_PROTO: &str = "application/connect+proto";

#[derive(Debug, Clone)]
pub struct ConnectResponse<T>(pub T);

impl<T> ConnectResponse<T> {
    /// Extract the inner value from the ConnectResponse wrapper.
    /// This is useful for converting ConnectResponse back to the original type,
    /// particularly when bridging to Tonic handlers.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> IntoResponse for ConnectResponse<T>
where
    T: Message + Serialize,
{
    fn into_response(self) -> Response {
        // Use the format from the request (set during ConnectRequest extraction)
        let format = get_request_format();
        let use_proto = format == ContentFormat::Proto;

        println!("📤 Encoding response: format={:?}", format);

        let (body, content_type) = if use_proto {
            // Encode as protobuf with gRPC frame envelope
            // Frame format: [flags:1][length:4][payload:length]
            let mut buf = vec![0u8; 5]; // Start with 5-byte header (compression flag + length)
            self.0.encode(&mut buf).unwrap();
            let payload_len = (buf.len() - 5) as u32;
            buf[1..5].copy_from_slice(&payload_len.to_be_bytes());
            println!("  Protobuf encoded: {} bytes (+ 5 byte frame header)", payload_len);
            (buf, "application/grpc+proto")
        } else {
            // Encode as JSON (no frame envelope for unary Connect JSON)
            let body = serde_json::to_vec(&self.0).unwrap();
            println!("  JSON encoded: {} bytes", body.len());
            (body, APPLICATION_JSON)
        };

        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type),
            )
            .body(Body::from(body))
            .unwrap()
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

/// IntoResponse implementation for streaming responses.
/// When T is wrapped in StreamBody, we encode it as a Connect streaming response.
impl<S, T> IntoResponse for ConnectResponse<StreamBody<S>>
where
    S: Stream<Item = Result<T, ConnectError>> + Send + 'static,
    T: Message + Serialize + Send + 'static,
{
    fn into_response(self) -> Response {
        use futures::StreamExt;

        let format = get_request_format();
        let use_proto = format == ContentFormat::Proto;

        println!("📤 Starting streaming response: format={:?}", format);

        // gRPC streaming: Just send message frames, no EndStreamResponse
        // Connect streaming: Send message frames + EndStreamResponse with flag 0x02
        // For now, since we detected gRPC by content-type, let's not send EndStreamResponse for gRPC

        let body_stream = self
            .0
            .stream
            .map(move |result| match result {
                Ok(msg) => {
                    // Regular message frame with flags=0x00
                    let mut buf = vec![0u8; 5];
                    if use_proto {
                        msg.encode(&mut buf).unwrap();
                    } else {
                        serde_json::to_writer(&mut buf, &msg).unwrap();
                    }
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    println!("  Sending message frame: {} bytes", buf.len());
                    Ok(Bytes::from(buf))
                }
                Err(err) => {
                    // For gRPC: errors should be sent as trailers, not in-stream
                    // For Connect: send Error EndStreamResponse with flags=0x02
                    // For simplicity, we'll send Connect-style error for now
                    println!("  Stream error: {:?}", err);
                    let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                    let json = serde_json::json!({ "error": err });
                    serde_json::to_writer(&mut buf, &json).unwrap();
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    Err(Bytes::from(buf)) // Mark as error to prevent success EndStream
                }
            })
            // Take all messages (including errors)
            .scan(false, |error_seen, item| {
                if *error_seen {
                    futures::future::ready(None)
                } else {
                    match &item {
                        Err(_) => {
                            *error_seen = true;
                            futures::future::ready(Some(item))
                        }
                        Ok(_) => futures::future::ready(Some(item)),
                    }
                }
            })
            // Convert Result<Bytes, Bytes> to Result<Bytes, Infallible>
            .map(|item| match item {
                Ok(bytes) => Ok::<_, std::convert::Infallible>(bytes),
                Err(bytes) => Ok::<_, std::convert::Infallible>(bytes),
            });

        let body = Body::from_stream(body_stream);

        let content_type = if use_proto {
            "application/grpc+proto"
        } else {
            APPLICATION_CONNECT_JSON
        };

        println!("  Response Content-Type: {}", content_type);

        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type),
            )
            .body(body)
            .unwrap()
    }
}

// Note: IntoResponse for Result<ConnectResponse<T>, ConnectError> is implemented by Axum's default implementation
