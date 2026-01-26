//! Connect streaming frame encoding.
//!
//! This module provides [`FrameEncoder`]: A stream adapter that encodes messages
//! into Connect protocol envelope frames.

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use connectrpc_axum_core::{
    CompressionConfig, CompressionEncoding, ENVELOPE_HEADER_SIZE, compress_payload, envelope_flags,
    wrap_envelope,
};

use crate::ClientError;
use futures::Stream;
use prost::Message;
use serde::Serialize;

/// State of the frame encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncoderState {
    /// Encoding messages from the inner stream.
    Streaming,
    /// Need to send the EndStream frame.
    SendEndStream,
    /// All frames have been sent.
    Done,
}

/// Stream adapter that encodes messages into Connect protocol envelope frames.
///
/// Wraps a stream of messages and yields framed bytes suitable for a streaming
/// request body.
///
/// # Frame Format
///
/// Connect streaming uses envelope framing:
/// ```text
/// [flags:1][length:4][payload:length]
/// ```
///
/// Flags:
/// - `0x00`: Uncompressed message
/// - `0x01`: Compressed message
/// - `0x02`: End of stream
///
/// # Example
///
/// ```ignore
/// use futures::stream;
///
/// let messages = stream::iter(vec![
///     MyMessage { value: "hello".into() },
///     MyMessage { value: "world".into() },
/// ]);
///
/// let encoder = FrameEncoder::new(
///     messages,
///     true,
///     CompressionEncoding::Identity,
///     CompressionConfig::disabled(),
/// );
///
/// // Use with reqwest::Body::wrap_stream(encoder)
/// ```
pub struct FrameEncoder<S, T> {
    /// The underlying message stream.
    stream: S,
    /// Use protobuf (true) or JSON (false) encoding.
    use_proto: bool,
    /// Compression encoding to use.
    encoding: CompressionEncoding,
    /// Compression configuration (min_bytes threshold and level).
    compression: CompressionConfig,
    /// Current encoder state.
    state: EncoderState,
    /// Type marker for the message type.
    _marker: PhantomData<T>,
}

impl<S, T> FrameEncoder<S, T> {
    /// Create a new frame encoder.
    ///
    /// # Arguments
    ///
    /// * `stream` - The underlying message stream
    /// * `use_proto` - Whether to use protobuf (true) or JSON (false) encoding
    /// * `encoding` - Compression encoding to use for outgoing messages
    /// * `compression` - Compression configuration (min_bytes threshold and level)
    pub fn new(
        stream: S,
        use_proto: bool,
        encoding: CompressionEncoding,
        compression: CompressionConfig,
    ) -> Self {
        Self {
            stream,
            use_proto,
            encoding,
            compression,
            state: EncoderState::Streaming,
            _marker: PhantomData,
        }
    }

    /// Get the compression encoding used by this encoder.
    pub fn encoding(&self) -> CompressionEncoding {
        self.encoding
    }

    /// Check if the encoder has finished sending all frames.
    pub fn is_finished(&self) -> bool {
        self.state == EncoderState::Done
    }

    /// Encode a message to bytes.
    fn encode_message(&self, msg: &T) -> Result<Bytes, ClientError>
    where
        T: Message + Serialize,
    {
        if self.use_proto {
            Ok(Bytes::from(msg.encode_to_vec()))
        } else {
            serde_json::to_vec(msg)
                .map(Bytes::from)
                .map_err(|e| ClientError::Encode(format!("JSON encoding failed: {}", e)))
        }
    }

    /// Encode a message into a framed envelope.
    fn encode_frame(&self, msg: &T) -> Result<Bytes, ClientError>
    where
        T: Message + Serialize,
    {
        // 1. Encode message
        let payload = self.encode_message(msg)?;

        // 2. Maybe compress
        let codec = if !self.encoding.is_identity()
            && !self.compression.is_disabled()
            && payload.len() >= self.compression.min_bytes
        {
            self.encoding.codec_with_level(self.compression.level)
        } else {
            None
        };

        let (payload, compressed) = compress_payload(payload, codec.as_ref())?;

        // 3. Wrap in envelope
        let frame = wrap_envelope(&payload, compressed);

        Ok(Bytes::from(frame))
    }

    /// Create the EndStream frame.
    ///
    /// The EndStream frame signals the end of the message stream.
    /// It contains a JSON payload that may include metadata or error info.
    fn end_stream_frame() -> Bytes {
        // Simple EndStream with empty JSON object
        let payload = b"{}";
        let mut frame = Vec::with_capacity(ENVELOPE_HEADER_SIZE + payload.len());
        frame.push(envelope_flags::END_STREAM);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        Bytes::from(frame)
    }
}

impl<S, T> Unpin for FrameEncoder<S, T> where S: Unpin {}

impl<S, T> Stream for FrameEncoder<S, T>
where
    S: Stream<Item = T> + Unpin,
    T: Message + Serialize,
{
    type Item = Result<Bytes, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match this.state {
                EncoderState::Streaming => {
                    // Poll the underlying stream for the next message
                    match Pin::new(&mut this.stream).poll_next(cx) {
                        Poll::Ready(Some(msg)) => {
                            // Encode the message into a frame
                            match this.encode_frame(&msg) {
                                Ok(frame) => return Poll::Ready(Some(Ok(frame))),
                                Err(e) => {
                                    // On error, mark as done and return the error
                                    this.state = EncoderState::Done;
                                    return Poll::Ready(Some(Err(e)));
                                }
                            }
                        }
                        Poll::Ready(None) => {
                            // Inner stream exhausted, need to send EndStream
                            this.state = EncoderState::SendEndStream;
                            // Continue to next iteration
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                EncoderState::SendEndStream => {
                    // Send the EndStream frame
                    this.state = EncoderState::Done;
                    return Poll::Ready(Some(Ok(Self::end_stream_frame())));
                }
                EncoderState::Done => {
                    // No more frames
                    return Poll::Ready(None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc_axum_core::CompressionEncoding;
    use futures::StreamExt;
    use futures::stream;

    // A simple test message type that implements both Message and Serialize
    #[derive(Clone, PartialEq, Default)]
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

    impl serde::Serialize for TestMessage {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use serde::ser::SerializeStruct;
            let mut state = serializer.serialize_struct("TestMessage", 1)?;
            state.serialize_field("value", &self.value)?;
            state.end()
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
    async fn test_encode_single_json_message() {
        let messages = stream::iter(vec![TestMessage {
            value: "hello".to_string(),
        }]);

        let mut encoder = FrameEncoder::new(
            messages,
            false, // JSON
            CompressionEncoding::Identity,
            CompressionConfig::disabled(),
        );

        // First frame should be the message
        let frame = encoder.next().await.unwrap().unwrap();

        // Parse the frame
        assert_eq!(frame[0], 0x00); // flags: uncompressed message
        let length = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;
        let payload = &frame[5..5 + length];
        assert_eq!(payload, br#"{"value":"hello"}"#);

        // Second frame should be EndStream
        let end_frame = encoder.next().await.unwrap().unwrap();
        assert_eq!(end_frame[0], 0x02); // flags: end stream

        // Should be done
        assert!(encoder.next().await.is_none());
        assert!(encoder.is_finished());
    }

    #[tokio::test]
    async fn test_encode_multiple_messages() {
        let messages = stream::iter(vec![
            TestMessage {
                value: "one".to_string(),
            },
            TestMessage {
                value: "two".to_string(),
            },
        ]);

        let mut encoder = FrameEncoder::new(
            messages,
            false, // JSON
            CompressionEncoding::Identity,
            CompressionConfig::disabled(),
        );

        // First message
        let frame1 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame1[0], 0x00);
        let len1 = u32::from_be_bytes([frame1[1], frame1[2], frame1[3], frame1[4]]) as usize;
        let payload1 = &frame1[5..5 + len1];
        assert_eq!(payload1, br#"{"value":"one"}"#);

        // Second message
        let frame2 = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame2[0], 0x00);
        let len2 = u32::from_be_bytes([frame2[1], frame2[2], frame2[3], frame2[4]]) as usize;
        let payload2 = &frame2[5..5 + len2];
        assert_eq!(payload2, br#"{"value":"two"}"#);

        // EndStream
        let end_frame = encoder.next().await.unwrap().unwrap();
        assert_eq!(end_frame[0], 0x02);

        // Done
        assert!(encoder.next().await.is_none());
    }

    #[tokio::test]
    async fn test_encode_proto_message() {
        let messages = stream::iter(vec![TestMessage {
            value: "hello".to_string(),
        }]);

        let mut encoder = FrameEncoder::new(
            messages,
            true, // Proto
            CompressionEncoding::Identity,
            CompressionConfig::disabled(),
        );

        // First frame should be the proto-encoded message
        let frame = encoder.next().await.unwrap().unwrap();
        assert_eq!(frame[0], 0x00); // uncompressed

        // Decode the proto payload
        let length = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;
        let payload = &frame[5..5 + length];
        let decoded = TestMessage::decode(payload).unwrap();
        assert_eq!(decoded.value, "hello");

        // EndStream
        let end_frame = encoder.next().await.unwrap().unwrap();
        assert_eq!(end_frame[0], 0x02);

        assert!(encoder.next().await.is_none());
    }

    #[tokio::test]
    async fn test_encode_empty_stream() {
        let messages = stream::iter(Vec::<TestMessage>::new());

        let mut encoder = FrameEncoder::new(
            messages,
            false,
            CompressionEncoding::Identity,
            CompressionConfig::disabled(),
        );

        // Should only get EndStream
        let end_frame = encoder.next().await.unwrap().unwrap();
        assert_eq!(end_frame[0], 0x02);

        // Done
        assert!(encoder.next().await.is_none());
    }
}
