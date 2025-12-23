//! Stream response wrapper for handling Connect RPC server-streaming.
//!
//! This module provides the `ConnectStreamResponse` wrapper for handling
//! server-streaming RPCs in the Connect protocol. The streaming logic is
//! implemented in the `ConnectStreamResponse` type's `IntoResponse` trait, which is
//! the idiomatic way to handle streaming responses in Axum.

use crate::error::ConnectError;
use crate::protocol::get_request_protocol;
use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use futures::Stream;
use prost::Message;
use serde::Serialize;

/// A response wrapper for server-streaming handlers.
///
/// This wrapper takes a stream of messages and encodes them according to the
/// Connect protocol for server streams. The encoding format (JSON or protobuf)
/// is determined by the incoming request's Content-Type.
#[derive(Debug)]
pub struct ConnectStreamResponse<S> {
    stream: S,
}

impl<S> ConnectStreamResponse<S> {
    /// Create a new `ConnectStreamResponse` from a stream of messages.
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    /// Extract the underlying stream from the response wrapper.
    ///
    /// This is useful for adapters that need to convert the stream to different formats,
    /// such as the gRPC adapter that needs to map ConnectError to tonic::Status.
    pub fn into_inner(self) -> S {
        self.stream
    }
}

impl<S, T> IntoResponse for ConnectStreamResponse<S>
where
    S: Stream<Item = Result<T, ConnectError>> + Send + 'static,
    T: Message + Serialize + Send + 'static,
{
    fn into_response(self) -> Response {
        use axum::http::StatusCode;
        use futures::StreamExt;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        // Get protocol from task-local (set by ConnectLayer middleware)
        let protocol = get_request_protocol();
        let use_proto = protocol.is_proto();

        // Connect streaming: Send message frames + EndStreamResponse with flag 0x02
        // Note: gRPC is handled by Tonic via ContentTypeSwitch

        // Track if an error was sent (for EndStream handling)
        let error_sent = Arc::new(AtomicBool::new(false));
        let error_sent_clone = error_sent.clone();

        let body_stream = self
            .stream
            .map(move |result| match result {
                Ok(msg) => {
                    // Regular message frame with flags=0x00
                    let mut buf = vec![0u8; 5];
                    if use_proto {
                        msg.encode(&mut buf).expect("protobuf encode failed");
                    } else {
                        serde_json::to_writer(&mut buf, &msg).expect("json encode failed");
                    }
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    (Bytes::from(buf), false)
                }
                Err(err) => {
                    // Error EndStreamResponse with flags=0x02 (EndStream)
                    let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                    let json = serde_json::json!({ "error": err });
                    serde_json::to_writer(&mut buf, &json).expect("json encode failed");
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    (Bytes::from(buf), true) // Mark as error
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
                    let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                    serde_json::to_writer(&mut buf, &serde_json::json!({}))
                        .expect("json encode failed");
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
                HeaderValue::from_static(protocol.response_content_type()),
            )
            .body(body)
            .expect("response build failed")
    }
}
