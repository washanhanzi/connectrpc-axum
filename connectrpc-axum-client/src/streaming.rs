//! Streaming response wrapper for Connect streaming responses.
//!
//! This module provides [`Streaming`], a wrapper around streaming response
//! bodies that provides access to trailers after the stream is consumed.
//!
//! # Cancellation
//!
//! Dropping a [`Streaming`] cancels the streaming RPC. The underlying HTTP
//! connection is closed, which signals cancellation to the server via TCP RST
//! or HTTP/2 RST_STREAM frame.
//!
//! For cooperative cancellation with timeouts, use [`CallOptions::timeout`]
//! which sends the `Connect-Timeout-Ms` header to the server.
//!
//! [`CallOptions::timeout`]: crate::CallOptions::timeout

use std::pin::Pin;
use std::task::{Context, Poll};

use crate::ClientError;
use futures::Stream;

use crate::frame::FrameDecoder;
use crate::response::Metadata;

/// Wrapper for streaming response messages.
///
/// `Streaming<S>` wraps a [`FrameDecoder`] and provides access to trailers after
/// the stream is fully consumed. This type is analogous to `tonic::Streaming<T>`.
///
/// # Cancellation
///
/// Dropping a `Streaming` cancels the streaming RPC. The underlying HTTP
/// connection is closed, signaling cancellation to the server. This is the
/// recommended way to cancel a stream when you no longer need more messages.
///
/// For cancellation with a timeout, configure the client or use per-call options:
///
/// ```ignore
/// use connectrpc_axum_client::CallOptions;
/// use std::time::Duration;
///
/// let options = CallOptions::new().timeout(Duration::from_secs(10));
/// let response = client.call_server_stream_with_options::<Req, Res>(
///     "service/Method", &request, options,
/// ).await?;
/// ```
///
/// # Example
///
/// ```ignore
/// let response = client.call_server_stream::<Req, Res>("pkg.Service/Method", &req).await?;
/// let mut stream = response.into_inner();
///
/// while let Some(result) = stream.next().await {
///     match result {
///         Ok(msg) => println!("Got message: {:?}", msg),
///         Err(e) => eprintln!("Error: {:?}", e),
///     }
/// }
///
/// // After stream is consumed, trailers are available
/// if let Some(trailers) = stream.trailers() {
///     println!("Trailers: {:?}", trailers);
/// }
/// ```
pub struct Streaming<S> {
    /// The underlying frame decoder.
    inner: S,
}

impl<S> Streaming<S> {
    /// Create a new Streaming wrapping the given stream.
    pub fn new(inner: S) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner stream.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner stream.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner stream.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, T> Streaming<FrameDecoder<S, T>> {
    /// Get the trailers received in the EndStream frame.
    ///
    /// Returns `None` if the stream hasn't finished or if no trailers were sent.
    ///
    /// Note: Trailers are only available after the stream has been fully consumed.
    pub fn trailers(&self) -> Option<&Metadata> {
        self.inner.trailers()
    }

    /// Take the trailers, leaving `None` in place.
    pub fn take_trailers(&mut self) -> Option<Metadata> {
        self.inner.take_trailers()
    }

    /// Check if the stream has finished.
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

/// Graceful shutdown methods for streaming responses.
impl<S, T> Streaming<S>
where
    S: Stream<Item = Result<T, ClientError>> + Unpin,
{
    /// Gracefully drain all remaining messages from the stream.
    ///
    /// This method consumes all remaining messages without processing them,
    /// allowing for graceful connection cleanup and reuse. After draining,
    /// trailers will be available via [`trailers()`](Streaming::trailers)
    /// if the inner stream is a `FrameDecoder`.
    ///
    /// Returns the number of messages that were drained (not including errors).
    ///
    /// # When to Use
    ///
    /// Use `drain()` when you want to:
    /// - Gracefully close a stream without processing remaining messages
    /// - Ensure connection reuse in HTTP/2
    /// - Access trailers after deciding to stop processing early
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = client.call_server_stream::<Req, Res>(...).await?;
    /// let mut stream = response.into_inner();
    ///
    /// // Process some messages
    /// for _ in 0..5 {
    ///     if let Some(Ok(msg)) = stream.next().await {
    ///         if should_stop(&msg) {
    ///             break;
    ///         }
    ///         process(msg);
    ///     }
    /// }
    ///
    /// // Gracefully drain remaining messages
    /// let drained = stream.drain().await;
    /// println!("Drained {} remaining messages", drained);
    ///
    /// // Trailers are now available
    /// if let Some(trailers) = stream.trailers() {
    ///     println!("Trailers: {:?}", trailers);
    /// }
    /// ```
    pub async fn drain(&mut self) -> usize {
        use futures::StreamExt;
        let mut count = 0;
        while let Some(result) = self.inner.next().await {
            if result.is_ok() {
                count += 1;
            }
        }
        count
    }

    /// Gracefully drain remaining messages with a timeout.
    ///
    /// Like [`drain()`](Self::drain), but returns early if the timeout expires.
    /// This prevents hanging indefinitely on slow or stuck streams.
    ///
    /// Returns `Ok(count)` if the stream was fully drained, or `Err(count)`
    /// if the timeout expired (where `count` is the number of messages drained
    /// before the timeout).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// let mut stream = response.into_inner();
    ///
    /// // Drain with a 5-second timeout
    /// match stream.drain_timeout(Duration::from_secs(5)).await {
    ///     Ok(count) => println!("Fully drained {} messages", count),
    ///     Err(count) => println!("Timeout after draining {} messages", count),
    /// }
    /// ```
    pub async fn drain_timeout(&mut self, timeout: std::time::Duration) -> Result<usize, usize> {
        use futures::StreamExt;
        let mut count = 0;
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            tokio::select! {
                biased;

                _ = tokio::time::sleep_until(deadline) => {
                    return Err(count);
                }

                item = self.inner.next() => {
                    match item {
                        Some(Ok(_)) => count += 1,
                        Some(Err(_)) => {}
                        None => return Ok(count),
                    }
                }
            }
        }
    }
}

impl<S, T> Stream for Streaming<S>
where
    S: Stream<Item = Result<T, ClientError>> + Unpin,
{
    type Item = Result<T, ClientError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use connectrpc_axum_core::CompressionEncoding;
    use futures::stream;
    use futures::StreamExt;

    // Helper to create a frame
    fn make_frame(flags: u8, payload: &[u8]) -> Bytes {
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(flags);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        Bytes::from(frame)
    }

    // A simple test message type that implements both Message and Deserialize
    #[derive(Clone, PartialEq)]
    struct TestMessage {
        value: String,
    }

    impl std::fmt::Debug for TestMessage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TestMessage")
                .field("value", &self.value)
                .finish()
        }
    }

    impl Default for TestMessage {
        fn default() -> Self {
            Self {
                value: String::new(),
            }
        }
    }

    impl<'de> serde::Deserialize<'de> for TestMessage {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            #[derive(serde::Deserialize)]
            struct Helper {
                value: String,
            }
            let helper = Helper::deserialize(deserializer)?;
            Ok(TestMessage { value: helper.value })
        }
    }

    impl prost::Message for TestMessage {
        fn encode_raw(&self, buf: &mut impl bytes::BufMut)
        where
            Self: Sized,
        {
            if !self.value.is_empty() {
                prost::encoding::string::encode(1, &self.value, buf);
            }
        }

        fn merge_field(
            &mut self,
            tag: u32,
            wire_type: prost::encoding::WireType,
            buf: &mut impl bytes::Buf,
            ctx: prost::encoding::DecodeContext,
        ) -> Result<(), prost::DecodeError>
        where
            Self: Sized,
        {
            if tag == 1 {
                prost::encoding::string::merge(wire_type, &mut self.value, buf, ctx)
            } else {
                prost::encoding::skip_field(wire_type, tag, buf, ctx)
            }
        }

        fn encoded_len(&self) -> usize {
            if self.value.is_empty() {
                0
            } else {
                prost::encoding::string::encoded_len(1, &self.value)
            }
        }

        fn clear(&mut self) {
            self.value.clear();
        }
    }

    #[tokio::test]
    async fn test_streaming_wraps_decoder() {
        let frame = make_frame(0x00, br#"{"value":"hello"}"#);
        let end_frame = make_frame(0x02, b"{}");

        let mut all_data = frame.to_vec();
        all_data.extend_from_slice(&end_frame);

        let byte_stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let decoder = FrameDecoder::<_, TestMessage>::new(
            byte_stream,
            false,
            CompressionEncoding::Identity,
        );
        let mut streaming = Streaming::new(decoder);

        let msg = streaming.next().await.unwrap().unwrap();
        assert_eq!(msg.value, "hello");

        assert!(streaming.next().await.is_none());
        assert!(streaming.is_finished());
    }

    #[tokio::test]
    async fn test_streaming_trailers() {
        let frame = make_frame(0x00, br#"{"value":"test"}"#);
        let end_payload = br#"{"metadata":{"x-custom":["value"]}}"#;
        let end_frame = make_frame(0x02, end_payload);

        let mut all_data = frame.to_vec();
        all_data.extend_from_slice(&end_frame);

        let byte_stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let decoder = FrameDecoder::<_, TestMessage>::new(
            byte_stream,
            false,
            CompressionEncoding::Identity,
        );
        let mut streaming = Streaming::new(decoder);

        // Consume stream
        while streaming.next().await.is_some() {}

        // Check trailers
        let trailers = streaming.trailers().unwrap();
        assert_eq!(trailers.get("x-custom"), Some("value"));
    }

    #[tokio::test]
    async fn test_streaming_drain() {
        // Create multiple message frames and end frame
        let frame1 = make_frame(0x00, br#"{"value":"msg1"}"#);
        let frame2 = make_frame(0x00, br#"{"value":"msg2"}"#);
        let frame3 = make_frame(0x00, br#"{"value":"msg3"}"#);
        let end_frame = make_frame(0x02, b"{}");

        let mut all_data = Vec::new();
        all_data.extend_from_slice(&frame1);
        all_data.extend_from_slice(&frame2);
        all_data.extend_from_slice(&frame3);
        all_data.extend_from_slice(&end_frame);

        let byte_stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let decoder = FrameDecoder::<_, TestMessage>::new(
            byte_stream,
            false,
            CompressionEncoding::Identity,
        );
        let mut streaming = Streaming::new(decoder);

        // Read first message
        let msg = streaming.next().await.unwrap().unwrap();
        assert_eq!(msg.value, "msg1");

        // Drain remaining messages (should drain msg2 and msg3)
        let drained = streaming.drain().await;
        assert_eq!(drained, 2);

        // Stream should be finished
        assert!(streaming.is_finished());
    }

    #[tokio::test]
    async fn test_streaming_drain_timeout() {
        // Create a frame and end frame
        let frame1 = make_frame(0x00, br#"{"value":"msg1"}"#);
        let end_frame = make_frame(0x02, b"{}");

        let mut all_data = Vec::new();
        all_data.extend_from_slice(&frame1);
        all_data.extend_from_slice(&end_frame);

        let byte_stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let decoder = FrameDecoder::<_, TestMessage>::new(
            byte_stream,
            false,
            CompressionEncoding::Identity,
        );
        let mut streaming = Streaming::new(decoder);

        // Drain with timeout (should complete quickly since stream is finite)
        let result = streaming
            .drain_timeout(std::time::Duration::from_secs(5))
            .await;
        assert_eq!(result, Ok(1)); // One message drained

        // Stream should be finished
        assert!(streaming.is_finished());
    }
}
