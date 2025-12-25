//! Response types for Connect.
use crate::error::{ConnectError, internal_error_response, internal_error_end_stream_frame, internal_error_streaming_response};
use crate::protocol::get_request_protocol;
use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::Stream;
use prost::Message;
use serde::Serialize;

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
        // Get protocol from task-local (set by ConnectLayer middleware)
        let protocol = get_request_protocol();

        let body = if protocol.is_proto() {
            // Connect unary proto: raw bytes, no frame envelope
            self.0.encode_to_vec()
        } else {
            // Connect unary JSON: raw JSON, no frame envelope
            match serde_json::to_vec(&self.0) {
                Ok(bytes) => bytes,
                Err(_) => {
                    // Serialization failed (e.g., non-finite floats, custom serializer errors)
                    return internal_error_response(protocol.error_content_type());
                }
            }
        };

        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static(protocol.response_content_type()),
            )
            .body(Body::from(body))
            .unwrap_or_else(|_| internal_error_response(protocol.error_content_type()))
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
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        // Get protocol from task-local (set by ConnectLayer middleware)
        let protocol = get_request_protocol();
        let use_proto = protocol.is_proto();
        let content_type = protocol.streaming_response_content_type();

        // Connect streaming: Send message frames + EndStreamResponse with flag 0x02
        // Note: gRPC is handled by Tonic via ContentTypeSwitch

        // Track if an error was sent (for EndStream handling)
        let error_sent = Arc::new(AtomicBool::new(false));
        let error_sent_clone = error_sent.clone();

        let body_stream = self
            .0
            .stream
            .map(move |result| match result {
                Ok(msg) => {
                    // Regular message frame with flags=0x00
                    let mut buf = vec![0u8; 5];
                    let encode_result = if use_proto {
                        msg.encode(&mut buf).map_err(|_| ())
                    } else {
                        serde_json::to_writer(&mut buf, &msg).map_err(|_| ())
                    };

                    match encode_result {
                        Ok(()) => {
                            let len = (buf.len() - 5) as u32;
                            buf[1..5].copy_from_slice(&len.to_be_bytes());
                            (Bytes::from(buf), false)
                        }
                        Err(()) => {
                            // Encoding failed - send internal error EndStream frame
                            (Bytes::from(internal_error_end_stream_frame()), true)
                        }
                    }
                }
                Err(err) => {
                    // Send Error EndStreamResponse with flags=0x02
                    let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                    let json = serde_json::json!({ "error": err });
                    match serde_json::to_writer(&mut buf, &json) {
                        Ok(()) => {
                            let len = (buf.len() - 5) as u32;
                            buf[1..5].copy_from_slice(&len.to_be_bytes());
                            (Bytes::from(buf), true)
                        }
                        Err(_) => {
                            // Error serialization failed - use fallback internal error frame
                            (Bytes::from(internal_error_end_stream_frame()), true)
                        }
                    }
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
            .chain(futures::stream::once(async move {
                if error_sent_clone.load(Ordering::SeqCst) {
                    // Error already sent EndStream
                    None
                } else {
                    // Connect streaming: append success EndStreamResponse
                    // Note: Serializing {} cannot fail in serde_json
                    let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                    let _ = serde_json::to_writer(&mut buf, &serde_json::json!({}));
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    Some(Bytes::from(buf))
                }
            }).filter_map(|x| async { x }))
            // Wrap in Result for Body::from_stream
            .map(|bytes| Ok::<_, std::convert::Infallible>(bytes));

        let body = Body::from_stream(body_stream);

        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                // Always use streaming content-type for StreamBody responses,
                // even if the request was unary (server-streaming case)
                HeaderValue::from_static(content_type),
            )
            .body(body)
            .unwrap_or_else(|_| internal_error_streaming_response(content_type))
    }
}

// Note: IntoResponse for Result<ConnectResponse<T>, ConnectError> is implemented by Axum's default implementation
