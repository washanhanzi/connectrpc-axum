//! Stream response wrapper for handling Connect RPC server-streaming.
//!
//! This module provides the `ConnectStreamResponse` wrapper for handling
//! server-streaming RPCs in the Connect protocol. The streaming logic is
//! implemented in the `ConnectStreamResponse` type's `IntoResponse` trait, which is
//! the idiomatic way to handle streaming responses in Axum.

use crate::error::ConnectError;
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
/// Connect protocol for server streams. It defaults to JSON encoding.
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

        // Track whether an error occurred to determine the final EndStreamResponse
        let body_stream = self
            .stream
            .map(|result| match result {
                Ok(msg) => {
                    // Regular message frame with flags=0x00
                    let mut buf = vec![0u8; 5];
                    serde_json::to_writer(&mut buf, &msg).unwrap();
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    Ok(Bytes::from(buf))
                }
                Err(err) => {
                    // Error EndStreamResponse with flags=0x02 (EndStream)
                    let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                    let json = serde_json::json!({ "error": err });
                    serde_json::to_writer(&mut buf, &json).unwrap();
                    let len = (buf.len() - 5) as u32;
                    buf[1..5].copy_from_slice(&len.to_be_bytes());
                    Err(Bytes::from(buf)) // Mark as error to prevent success EndStream
                }
            })
            .chain(futures::stream::once(async {
                // Success EndStreamResponse with flags=0x02 (EndStream)
                // This is sent only if no error occurred (handled by scan below)
                let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0];
                let json = serde_json::json!({});
                serde_json::to_writer(&mut buf, &json).unwrap();
                let len = (buf.len() - 5) as u32;
                buf[1..5].copy_from_slice(&len.to_be_bytes());
                Ok(Bytes::from(buf))
            }))
            // Take messages while they're Ok, stop after first Err (which contains the error EndStream)
            .scan(false, |error_seen, item| {
                if *error_seen {
                    futures::future::ready(None)
                } else {
                    match item {
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

        // Convert the stream directly to Body
        let body = Body::from_stream(body_stream);

        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/connect+json"),
            )
            .body(body)
            .unwrap()
    }
}
