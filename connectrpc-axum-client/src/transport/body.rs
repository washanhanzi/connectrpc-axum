//! Request body types for HTTP transport.
//!
//! This module provides [`TransportBody`], a unified body type for Connect RPC requests
//! that works with hyper's HTTP client.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Stream;
use http_body::{Body, Frame};
use pin_project_lite::pin_project;

use crate::ClientError;

pin_project! {
    /// A request body for Connect RPC calls.
    ///
    /// This type can represent:
    /// - Empty bodies (for some GET requests)
    /// - Full bodies (for unary requests with complete message)
    /// - Streaming bodies (for client/bidi streaming requests)
    #[project = TransportBodyProj]
    pub enum TransportBody {
        /// Empty request body.
        Empty,
        /// Full request body with all data available.
        Full {
            data: Option<Bytes>,
        },
        /// Streaming request body from an async stream.
        Streaming {
            #[pin]
            stream: Pin<Box<dyn Stream<Item = Result<Bytes, ClientError>> + Send>>,
        },
    }
}

impl TransportBody {
    /// Create an empty body.
    pub fn empty() -> Self {
        TransportBody::Empty
    }

    /// Create a body with the given data.
    pub fn full(data: Bytes) -> Self {
        TransportBody::Full { data: Some(data) }
    }

    /// Create a streaming body from the given stream.
    pub fn streaming<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, ClientError>> + Send + 'static,
    {
        TransportBody::Streaming {
            stream: Box::pin(stream),
        }
    }
}

impl Body for TransportBody {
    type Data = Bytes;
    type Error = ClientError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.project() {
            TransportBodyProj::Empty => Poll::Ready(None),
            TransportBodyProj::Full { data } => {
                let result = data.take().map(|d| Ok(Frame::data(d)));
                Poll::Ready(result)
            }
            TransportBodyProj::Streaming { stream } => {
                match stream.poll_next(cx) {
                    Poll::Ready(Some(Ok(data))) => Poll::Ready(Some(Ok(Frame::data(data)))),
                    Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
                    Poll::Ready(None) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            TransportBody::Empty => true,
            TransportBody::Full { data } => data.is_none(),
            TransportBody::Streaming { .. } => false, // Can't know without polling
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match self {
            TransportBody::Empty => http_body::SizeHint::with_exact(0),
            TransportBody::Full { data } => {
                if let Some(d) = data {
                    http_body::SizeHint::with_exact(d.len() as u64)
                } else {
                    http_body::SizeHint::with_exact(0)
                }
            }
            TransportBody::Streaming { .. } => http_body::SizeHint::default(),
        }
    }
}

impl Default for TransportBody {
    fn default() -> Self {
        TransportBody::Empty
    }
}

impl std::fmt::Debug for TransportBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportBody::Empty => write!(f, "TransportBody::Empty"),
            TransportBody::Full { data } => f
                .debug_struct("TransportBody::Full")
                .field("data_len", &data.as_ref().map(|d| d.len()))
                .finish(),
            TransportBody::Streaming { .. } => write!(f, "TransportBody::Streaming"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_empty_body() {
        let mut body = TransportBody::empty();
        assert!(body.is_end_stream());

        let collected = Pin::new(&mut body).collect().await.unwrap();
        assert!(collected.to_bytes().is_empty());
    }

    #[tokio::test]
    async fn test_full_body() {
        let data = Bytes::from("hello world");
        let mut body = TransportBody::full(data.clone());

        let collected = Pin::new(&mut body).collect().await.unwrap();
        assert_eq!(collected.to_bytes(), data);
    }

    #[tokio::test]
    async fn test_streaming_body() {
        let chunks = vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
            Ok(Bytes::from("chunk3")),
        ];
        let stream = futures::stream::iter(chunks);
        let mut body = TransportBody::streaming(stream);

        let collected = Pin::new(&mut body).collect().await.unwrap();
        assert_eq!(collected.to_bytes(), Bytes::from("chunk1chunk2chunk3"));
    }
}
