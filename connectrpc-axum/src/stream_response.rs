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
use futures::{Stream, TryStreamExt};
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
        let body_stream = self
            .stream
            .map_ok(|msg| {
                let mut buf = vec![0u8; 5];
                serde_json::to_writer(&mut buf, &msg).unwrap();
                let len = (buf.len() - 5) as u32;
                buf[1..5].copy_from_slice(&len.to_be_bytes());
                Bytes::from(buf)
            })
            .or_else(|err| {
                let mut buf = vec![0b0000_0010u8, 0, 0, 0, 0]; // End stream flag
                let json = serde_json::json!({ "error": err });
                serde_json::to_writer(&mut buf, &json).unwrap();
                let len = (buf.len() - 5) as u32;
                buf[1..5].copy_from_slice(&len.to_be_bytes());
                futures::future::ready(Ok::<Bytes, std::convert::Infallible>(Bytes::from(buf)))
            });

        // Convert the stream directly to Body
        let body = Body::from_stream(body_stream);

        Response::builder()
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/connect+json"),
            )
            .body(body)
            .unwrap()
    }
}
