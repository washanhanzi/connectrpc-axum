//! Connect streaming frame encoding and decoding.
//!
//! This module provides:
//! - [`FrameDecoder`]: A stream adapter that parses Connect protocol envelope
//!   frames from a byte stream and yields decoded messages.
//! - [`FrameEncoder`]: A stream adapter that encodes messages into Connect
//!   protocol envelope frames.

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use base64::Engine;
use bytes::{Bytes, BytesMut};
use connectrpc_axum_core::{
    compress_payload, envelope_flags, parse_envelope_header, process_envelope_payload,
    wrap_envelope, Code, CompressionConfig, CompressionEncoding, ErrorDetail,
    ENVELOPE_HEADER_SIZE,
};

use crate::ClientError;
use futures::Stream;
use prost::Message;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::response::Metadata;

/// Decoded streaming frame result.
enum DecodedFrame<T> {
    /// A message frame containing a decoded message.
    Message(T),
    /// End of stream (trailers are stored in the decoder).
    EndStream,
}

/// Stream adapter that decodes Connect protocol envelope frames.
///
/// Wraps a byte stream (from `reqwest::Response::bytes_stream()`) and yields
/// decoded protobuf or JSON messages.
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
/// let stream = response.bytes_stream();
/// let decoder = FrameDecoder::<_, MyMessage>::new(stream, true, CompressionEncoding::Identity);
///
/// while let Some(result) = decoder.next().await {
///     match result? {
///         msg => println!("Got message: {:?}", msg),
///     }
/// }
/// ```
pub struct FrameDecoder<S, T> {
    /// The underlying byte stream.
    stream: S,
    /// Buffer for incomplete frames.
    buffer: BytesMut,
    /// Use protobuf (true) or JSON (false) decoding.
    use_proto: bool,
    /// Compression encoding for decompression.
    encoding: CompressionEncoding,
    /// Stored trailers from EndStream frame.
    trailers: Option<Metadata>,
    /// Whether the stream has finished (received EndStream or error).
    finished: bool,
    /// Error from the EndStream frame, if any.
    end_stream_error: Option<ClientError>,
    /// Type marker for the message type.
    _marker: PhantomData<T>,
}

impl<S, T> FrameDecoder<S, T> {
    /// Create a new frame decoder.
    ///
    /// # Arguments
    ///
    /// * `stream` - The underlying byte stream
    /// * `use_proto` - Whether to use protobuf (true) or JSON (false) decoding
    /// * `encoding` - Compression encoding for decompression
    pub fn new(stream: S, use_proto: bool, encoding: CompressionEncoding) -> Self {
        Self {
            stream,
            buffer: BytesMut::new(),
            use_proto,
            encoding,
            trailers: None,
            finished: false,
            end_stream_error: None,
            _marker: PhantomData,
        }
    }

    /// Get the trailers received in the EndStream frame.
    ///
    /// Returns `None` if the stream hasn't finished or if no trailers were sent.
    pub fn trailers(&self) -> Option<&Metadata> {
        self.trailers.as_ref()
    }

    /// Take the trailers, leaving `None` in place.
    pub fn take_trailers(&mut self) -> Option<Metadata> {
        self.trailers.take()
    }

    /// Check if the stream has finished.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Decode a message from bytes.
    fn decode_message(&self, bytes: &[u8]) -> Result<T, ClientError>
    where
        T: Message + DeserializeOwned + Default,
    {
        if self.use_proto {
            T::decode(bytes)
                .map_err(|e| ClientError::Decode(format!("protobuf decoding failed: {}", e)))
        } else {
            serde_json::from_slice(bytes)
                .map_err(|e| ClientError::Decode(format!("JSON decoding failed: {}", e)))
        }
    }

    /// Try to parse a complete frame from the buffer.
    ///
    /// Returns:
    /// - `Ok(Some(frame))` if a complete frame was parsed
    /// - `Ok(None)` if more data is needed
    /// - `Err(e)` if there was a parsing error
    fn try_parse_frame(&mut self) -> Result<Option<DecodedFrame<T>>, ClientError>
    where
        T: Message + DeserializeOwned + Default,
    {
        // Need at least the header
        if self.buffer.len() < ENVELOPE_HEADER_SIZE {
            return Ok(None);
        }

        // Parse header
        let (flags, length) = parse_envelope_header(&self.buffer)?;
        let frame_size = ENVELOPE_HEADER_SIZE + length as usize;

        // Check if we have the complete frame
        if self.buffer.len() < frame_size {
            return Ok(None);
        }

        // Extract frame bytes
        let frame_bytes = self.buffer.split_to(frame_size);
        let payload = Bytes::copy_from_slice(&frame_bytes[ENVELOPE_HEADER_SIZE..]);

        // Check if this is an EndStream frame
        if flags == envelope_flags::END_STREAM {
            let (error, trailers) = parse_end_stream(&payload)?;

            // Store trailers
            self.trailers = trailers;
            self.finished = true;

            if let Some(err) = error {
                // Store error for next poll
                self.end_stream_error = Some(err);
            }

            return Ok(Some(DecodedFrame::EndStream));
        }

        // Process message frame (validate flags, decompress)
        let decompressed = process_envelope_payload(flags, payload, self.encoding)?
            .ok_or_else(|| ClientError::Protocol("unexpected None from message frame".into()))?;

        // Decode message
        let message = self.decode_message(&decompressed)?;

        Ok(Some(DecodedFrame::Message(message)))
    }
}

impl<S, T> Unpin for FrameDecoder<S, T> where S: Unpin {}

impl<S, T> Stream for FrameDecoder<S, T>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
    T: Message + DeserializeOwned + Default,
{
    type Item = Result<T, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // Check for stored EndStream error
            if let Some(err) = this.end_stream_error.take() {
                return Poll::Ready(Some(Err(err)));
            }

            // If finished, no more items
            if this.finished {
                return Poll::Ready(None);
            }

            // Try to parse a frame from the buffer
            match this.try_parse_frame() {
                Ok(Some(DecodedFrame::Message(msg))) => {
                    return Poll::Ready(Some(Ok(msg)));
                }
                Ok(Some(DecodedFrame::EndStream)) => {
                    // Check for error from EndStream
                    if let Some(err) = this.end_stream_error.take() {
                        return Poll::Ready(Some(Err(err)));
                    }
                    // Successful end of stream
                    return Poll::Ready(None);
                }
                Ok(None) => {
                    // Need more data, poll the underlying stream
                }
                Err(e) => {
                    this.finished = true;
                    return Poll::Ready(Some(Err(e)));
                }
            }

            // Poll the underlying stream for more data
            match Pin::new(&mut this.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    this.buffer.extend_from_slice(&chunk);
                    // Loop back to try parsing again
                }
                Poll::Ready(Some(Err(e))) => {
                    this.finished = true;
                    return Poll::Ready(Some(Err(ClientError::Transport(format!(
                        "stream error: {}",
                        e
                    )))));
                }
                Poll::Ready(None) => {
                    // Stream ended unexpectedly
                    this.finished = true;
                    if !this.buffer.is_empty() {
                        return Poll::Ready(Some(Err(ClientError::new(
                            Code::DataLoss,
                            format!(
                                "stream ended with {} bytes of incomplete data",
                                this.buffer.len()
                            ),
                        ))));
                    }
                    // Stream ended cleanly but without EndStream frame - protocol error
                    return Poll::Ready(Some(Err(ClientError::Protocol(
                        "stream ended without EndStream frame".into(),
                    ))));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

/// EndStream frame JSON structure.
#[derive(Deserialize)]
struct EndStreamJson {
    #[serde(default)]
    error: Option<EndStreamError>,
    #[serde(default)]
    metadata: Option<std::collections::HashMap<String, Vec<String>>>,
}

/// Error structure in EndStream frame.
#[derive(Deserialize)]
struct EndStreamError {
    code: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    details: Vec<EndStreamErrorDetail>,
}

/// Error detail in EndStream frame.
#[derive(Deserialize)]
struct EndStreamErrorDetail {
    #[serde(rename = "type")]
    type_url: String,
    #[serde(default)]
    value: String,
}

/// Parse an EndStream frame payload.
///
/// Returns `(error, trailers)` where both are optional.
fn parse_end_stream(payload: &[u8]) -> Result<(Option<ClientError>, Option<Metadata>), ClientError> {
    // Empty payload is valid (no error, no trailers)
    if payload.is_empty() || payload == b"{}" {
        return Ok((None, None));
    }

    let end_stream: EndStreamJson = serde_json::from_slice(payload)
        .map_err(|e| ClientError::Protocol(format!("invalid EndStream JSON: {}", e)))?;

    // Parse error if present
    let error = end_stream.error.map(|e| {
        let code = e.code.parse().unwrap_or(Code::Unknown);
        let mut err = if let Some(msg) = e.message {
            ClientError::new(code, msg)
        } else {
            ClientError::from_code(code)
        };

        // Parse error details
        for detail in e.details {
            if let Some(parsed) = parse_error_detail(&detail) {
                err = err.add_error_detail(parsed);
            }
        }

        err
    });

    // Parse trailers/metadata if present
    let trailers = end_stream.metadata.map(|meta| {
        let mut headers = http::HeaderMap::new();
        for (key, values) in meta {
            if let Ok(name) = http::header::HeaderName::try_from(&key) {
                for value in values {
                    if let Ok(hv) = http::header::HeaderValue::try_from(&value) {
                        headers.append(name.clone(), hv);
                    }
                }
            }
        }
        Metadata::new(headers)
    });

    Ok((error, trailers))
}

/// Parse an error detail from EndStream JSON.
fn parse_error_detail(detail: &EndStreamErrorDetail) -> Option<ErrorDetail> {
    // Decode base64 value (Connect uses standard base64 without padding)
    let value = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(&detail.value)
        .or_else(|_| {
            // Also try with padding in case server sends it
            base64::engine::general_purpose::STANDARD.decode(&detail.value)
        })
        .ok()?;

    Some(ErrorDetail::new(&detail.type_url, value))
}

// ============================================================================
// FrameEncoder - Encodes messages into Connect protocol envelope frames
// ============================================================================

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
    use bytes::Bytes;
    use futures::StreamExt;
    use futures::stream;

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
    async fn test_decode_single_json_message() {
        let payload = br#"{"value":"hello"}"#;
        let frame = make_frame(0x00, payload);

        // Add EndStream frame
        let end_frame = make_frame(0x02, b"{}");

        let mut all_data = frame.to_vec();
        all_data.extend_from_slice(&end_frame);

        let stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            stream,
            false, // JSON
            CompressionEncoding::Identity,
        );

        let msg = decoder.next().await.unwrap().unwrap();
        assert_eq!(msg.value, "hello");

        // Should be done
        assert!(decoder.next().await.is_none());
    }

    #[tokio::test]
    async fn test_decode_multiple_messages() {
        let frame1 = make_frame(0x00, br#"{"value":"one"}"#);
        let frame2 = make_frame(0x00, br#"{"value":"two"}"#);
        let end_frame = make_frame(0x02, b"{}");

        let mut all_data = frame1.to_vec();
        all_data.extend_from_slice(&frame2);
        all_data.extend_from_slice(&end_frame);

        let stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            stream,
            false,
            CompressionEncoding::Identity,
        );

        let msg1 = decoder.next().await.unwrap().unwrap();
        assert_eq!(msg1.value, "one");

        let msg2 = decoder.next().await.unwrap().unwrap();
        assert_eq!(msg2.value, "two");

        assert!(decoder.next().await.is_none());
    }

    #[tokio::test]
    async fn test_decode_with_error_in_end_stream() {
        let frame = make_frame(0x00, br#"{"value":"hello"}"#);
        let end_payload = br#"{"error":{"code":"internal","message":"test error"}}"#;
        let end_frame = make_frame(0x02, end_payload);

        let mut all_data = frame.to_vec();
        all_data.extend_from_slice(&end_frame);

        let stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            stream,
            false,
            CompressionEncoding::Identity,
        );

        // First message should succeed
        let msg = decoder.next().await.unwrap().unwrap();
        assert_eq!(msg.value, "hello");

        // Next should be the error
        let err = decoder.next().await.unwrap().unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert_eq!(err.message(), Some("test error"));
    }

    #[tokio::test]
    async fn test_decode_with_trailers() {
        let frame = make_frame(0x00, br#"{"value":"hello"}"#);
        let end_payload = br#"{"metadata":{"x-custom":["value1","value2"]}}"#;
        let end_frame = make_frame(0x02, end_payload);

        let mut all_data = frame.to_vec();
        all_data.extend_from_slice(&end_frame);

        let stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_data))]);
        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            stream,
            false,
            CompressionEncoding::Identity,
        );

        // Consume message
        let _ = decoder.next().await;

        // Stream should end
        assert!(decoder.next().await.is_none());

        // Check trailers
        let trailers = decoder.trailers().unwrap();
        let values: Vec<_> = trailers.get_all("x-custom").collect();
        assert_eq!(values, vec!["value1", "value2"]);
    }

    #[tokio::test]
    async fn test_chunked_data() {
        // Split a frame across multiple chunks
        let payload = br#"{"value":"hello"}"#;
        let frame = make_frame(0x00, payload);
        let end_frame = make_frame(0x02, b"{}");

        let mut all_data = frame.to_vec();
        all_data.extend_from_slice(&end_frame);

        // Split into small chunks
        let chunk1 = Bytes::copy_from_slice(&all_data[..3]);
        let chunk2 = Bytes::copy_from_slice(&all_data[3..10]);
        let chunk3 = Bytes::copy_from_slice(&all_data[10..]);

        let stream = stream::iter(vec![
            Ok::<_, reqwest::Error>(chunk1),
            Ok(chunk2),
            Ok(chunk3),
        ]);

        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            stream,
            false,
            CompressionEncoding::Identity,
        );

        let msg = decoder.next().await.unwrap().unwrap();
        assert_eq!(msg.value, "hello");

        assert!(decoder.next().await.is_none());
    }

    #[test]
    fn test_parse_end_stream_empty() {
        let (error, trailers) = parse_end_stream(b"{}").unwrap();
        assert!(error.is_none());
        assert!(trailers.is_none());
    }

    #[test]
    fn test_parse_end_stream_with_error() {
        let payload = br#"{"error":{"code":"not_found","message":"resource not found"}}"#;
        let (error, trailers) = parse_end_stream(payload).unwrap();

        let err = error.unwrap();
        assert_eq!(err.code(), Code::NotFound);
        assert_eq!(err.message(), Some("resource not found"));
        assert!(trailers.is_none());
    }

    #[test]
    fn test_parse_end_stream_with_metadata() {
        let payload = br#"{"metadata":{"x-request-id":["123"]}}"#;
        let (error, trailers) = parse_end_stream(payload).unwrap();

        assert!(error.is_none());
        let meta = trailers.unwrap();
        assert_eq!(meta.get("x-request-id"), Some("123"));
    }

    // ========================================================================
    // FrameEncoder tests
    // ========================================================================

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

    #[tokio::test]
    async fn test_encode_and_decode_roundtrip() {
        // Create messages
        let original_messages = vec![
            TestMessage {
                value: "first".to_string(),
            },
            TestMessage {
                value: "second".to_string(),
            },
            TestMessage {
                value: "third".to_string(),
            },
        ];

        // Encode
        let encoder = FrameEncoder::new(
            stream::iter(original_messages.clone()),
            false, // JSON
            CompressionEncoding::Identity,
            CompressionConfig::disabled(),
        );

        // Collect all frames
        let frames: Vec<Bytes> = encoder
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        // Concatenate frames into a single byte stream
        let all_bytes: Vec<u8> = frames.iter().flat_map(|f| f.to_vec()).collect();

        // Decode
        let byte_stream =
            stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_bytes))]);
        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            byte_stream,
            false,
            CompressionEncoding::Identity,
        );

        // Verify decoded messages match originals
        for original in &original_messages {
            let decoded = decoder.next().await.unwrap().unwrap();
            assert_eq!(decoded.value, original.value);
        }

        // Should be done
        assert!(decoder.next().await.is_none());
    }

    // === Protocol Conformance Tests ===
    // These tests verify wire format compliance with the Connect protocol specification.

    /// Verify frame flags values per Connect protocol spec:
    /// - 0x00: Uncompressed message
    /// - 0x01: Compressed message
    /// - 0x02: End stream (uncompressed)
    /// - 0x03: End stream (compressed)
    #[test]
    fn test_conformance_frame_flags() {
        // Message frame uncompressed
        let msg_frame = make_frame(0x00, b"test");
        assert_eq!(msg_frame[0] & 0x01, 0x00, "Message flag should indicate uncompressed");
        assert_eq!(msg_frame[0] & 0x02, 0x00, "Message flag should not have end-stream bit");

        // End stream frame
        let end_frame = make_frame(0x02, b"{}");
        assert_eq!(end_frame[0] & 0x02, 0x02, "End stream flag should have end-stream bit");
    }

    /// Verify length encoding is big-endian 4-byte unsigned integer
    #[test]
    fn test_conformance_length_encoding() {
        // Test various lengths
        let payloads = [
            vec![0u8; 0],           // Empty
            vec![0u8; 1],           // Single byte
            vec![0u8; 255],         // Max single byte
            vec![0u8; 256],         // Two bytes needed
            vec![0u8; 65535],       // Max two bytes
        ];

        for payload in &payloads {
            let frame = make_frame(0x00, payload);

            // Extract length from bytes 1-4 (big endian)
            let encoded_length = u32::from_be_bytes([
                frame[1], frame[2], frame[3], frame[4]
            ]) as usize;

            assert_eq!(
                encoded_length, payload.len(),
                "Encoded length {} should match payload length {}",
                encoded_length, payload.len()
            );
        }
    }

    /// Verify end stream JSON structure per Connect protocol spec
    #[tokio::test]
    async fn test_conformance_end_stream_json_structure() {
        // Empty end stream (success)
        let end1 = make_frame(0x02, b"{}");
        let parsed: serde_json::Value = serde_json::from_slice(&end1[5..]).unwrap();
        assert!(parsed.is_object());

        // End stream with error
        let error_json = br#"{"error":{"code":"not_found","message":"Resource not found"}}"#;
        let end2 = make_frame(0x02, error_json);
        let parsed: serde_json::Value = serde_json::from_slice(&end2[5..]).unwrap();
        assert_eq!(parsed["error"]["code"], "not_found");

        // End stream with metadata
        let meta_json = br#"{"metadata":{"x-request-id":["abc123"]}}"#;
        let end3 = make_frame(0x02, meta_json);
        let parsed: serde_json::Value = serde_json::from_slice(&end3[5..]).unwrap();
        assert!(parsed["metadata"]["x-request-id"].is_array());
    }

    /// Verify message ordering is preserved in streaming
    #[tokio::test]
    async fn test_conformance_message_ordering() {
        // Create 10 messages with sequence numbers
        let messages: Vec<TestMessage> = (0..10)
            .map(|i| TestMessage {
                value: format!("msg_{}", i),
            })
            .collect();

        // Encode
        let encoder = FrameEncoder::new(
            stream::iter(messages.clone()),
            false,
            CompressionEncoding::Identity,
            CompressionConfig::disabled(),
        );

        let frames: Vec<Bytes> = encoder
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        let all_bytes: Vec<u8> = frames.iter().flat_map(|f| f.to_vec()).collect();

        // Decode and verify ordering
        let byte_stream = stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from(all_bytes))]);
        let mut decoder = FrameDecoder::<_, TestMessage>::new(
            byte_stream,
            false,
            CompressionEncoding::Identity,
        );

        for (i, original) in messages.iter().enumerate() {
            let decoded = decoder.next().await.unwrap().unwrap();
            assert_eq!(
                decoded.value, original.value,
                "Message {} should be in order", i
            );
        }
    }

    /// Verify minimum frame size (5 bytes: 1 flag + 4 length)
    #[test]
    fn test_conformance_minimum_frame_size() {
        let empty_frame = make_frame(0x00, b"");
        assert_eq!(empty_frame.len(), 5, "Empty payload frame should be exactly 5 bytes");
    }
}
