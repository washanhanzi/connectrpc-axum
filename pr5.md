# PR 5: Streaming Compression Support - Implementation Plan

## Overview

This PR adds Connect-specific compression support for streaming RPCs. Unlike unary compression (which uses standard HTTP `Content-Encoding` / `Accept-Encoding` headers), streaming compression:

1. Uses Connect-specific headers: `Connect-Content-Encoding` and `Connect-Accept-Encoding`
2. Operates at the **frame payload level**, not the HTTP body level
3. Each frame's flag byte indicates whether that specific frame is compressed

### Connect Protocol Frame Format

```
[flags: 1 byte][length: 4 bytes BE][payload: length bytes]
```

**Flag bits:**
- `0x01` - Compressed: payload is compressed using the advertised encoding
- `0x02` - EndStream: this is the final frame containing JSON metadata

### Header Mapping (Unary vs Streaming)

| Purpose | Unary Header | Streaming Header |
|---------|--------------|------------------|
| Request compression | `Content-Encoding` | `Connect-Content-Encoding` |
| Accepted compressions | `Accept-Encoding` | `Connect-Accept-Encoding` |

### Key Behaviors (from connect-go reference)

1. **Request Decompression**: Server reads `Connect-Content-Encoding` header and decompresses frames with flag 0x01
2. **Response Compression Negotiation**: Server reads `Connect-Accept-Encoding` header to determine response compression
3. **Per-Frame Response Compression**: Server compresses individual frame payloads when client supports it and payload exceeds threshold
4. **Unsupported Encoding Rejection**: Return error in EndStream frame with appropriate error code

## Dependencies

This PR assumes PR4 (Unary Compression) has been merged, providing:
- `compression.rs` module with `CompressionEncoding`, `CompressionConfig`, `compress()`, `decompress()`
- `context/compression.rs` with `CompressionContext`

If PR4 is not yet merged, those components need to be created as part of this PR.

## Implementation Steps

### Step 1: Create Streaming Compression Context

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/context/streaming_compression.rs`

Create a context type specifically for streaming compression:

```rust
//! Streaming compression context for request/response handling.

use crate::compression::{CompressionConfig, CompressionEncoding};

/// Streaming compression context extracted from request headers.
///
/// For streaming RPCs, compression headers are:
/// - `Connect-Content-Encoding`: encoding used for request frame payloads
/// - `Connect-Accept-Encoding`: encodings client accepts for response frames
#[derive(Debug, Clone, Copy)]
pub struct StreamingCompressionContext {
    /// The encoding used for compressed request frames (from Connect-Content-Encoding).
    pub request_encoding: Option<CompressionEncoding>,
    /// The encoding to use for response frames (negotiated from Connect-Accept-Encoding).
    pub response_encoding: CompressionEncoding,
    /// Compression configuration (min bytes threshold, enabled algorithms).
    pub config: CompressionConfig,
}

impl Default for StreamingCompressionContext {
    fn default() -> Self {
        Self {
            request_encoding: None,
            response_encoding: CompressionEncoding::Identity,
            config: CompressionConfig::default(),
        }
    }
}
```

**Note**: `request_encoding` is `Option` because the `Connect-Content-Encoding` header is optional. When present, frames with flag 0x01 are decompressed using this encoding. When absent, receiving a compressed frame (flag 0x01) is an error.

### Step 2: Add Streaming Header Parsing

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/layer/streaming_compression.rs`

```rust
//! Streaming compression negotiation for Connect RPC.
//!
//! Handles Connect-Content-Encoding and Connect-Accept-Encoding headers
//! for streaming RPCs.

use crate::compression::{CompressionConfig, CompressionEncoding, negotiate_response_encoding};
use crate::context::StreamingCompressionContext;
use crate::error::{Code, ConnectError};
use axum::http::{header::HeaderName, Request};

/// Connect-specific header for streaming request compression.
pub static CONNECT_CONTENT_ENCODING: HeaderName =
    HeaderName::from_static("connect-content-encoding");

/// Connect-specific header for streaming response compression negotiation.
pub static CONNECT_ACCEPT_ENCODING: HeaderName =
    HeaderName::from_static("connect-accept-encoding");

/// Validate Connect-Content-Encoding header for streaming requests.
///
/// Returns `Some(encoding)` if header is present and valid.
/// Returns error if encoding is present but unsupported.
pub fn validate_streaming_request_encoding(
    content_encoding: Option<&str>,
    config: &CompressionConfig,
) -> Result<Option<CompressionEncoding>, ConnectError> {
    match content_encoding {
        None | Some("") => Ok(None), // No compression declared
        Some(encoding_str) => {
            match CompressionEncoding::from_content_encoding(encoding_str) {
                Some(encoding) if config.supports(encoding) => Ok(Some(encoding)),
                Some(encoding) => {
                    Err(ConnectError::new(
                        Code::Unimplemented,
                        format!(
                            "unknown compression \"{}\": supported encodings are {}",
                            encoding.as_str(),
                            config.supported_encodings()
                        ),
                    ))
                }
                None => {
                    Err(ConnectError::new(
                        Code::Unimplemented,
                        format!(
                            "unknown compression \"{}\": supported encodings are {}",
                            encoding_str,
                            config.supported_encodings()
                        ),
                    ))
                }
            }
        }
    }
}

/// Extract streaming compression context from request headers.
pub fn extract_streaming_compression_context<B>(
    req: &Request<B>,
    config: &CompressionConfig,
) -> Result<StreamingCompressionContext, ConnectError> {
    // Get Connect-Content-Encoding header
    let content_encoding = req
        .headers()
        .get(&CONNECT_CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());

    // Validate request encoding
    let request_encoding = validate_streaming_request_encoding(content_encoding, config)?;

    // Get Connect-Accept-Encoding header and negotiate response compression
    let accept_encoding = req
        .headers()
        .get(&CONNECT_ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let response_encoding = negotiate_response_encoding(accept_encoding, config);

    Ok(StreamingCompressionContext {
        request_encoding,
        response_encoding,
        config: *config,
    })
}
```

### Step 3: Update Frame Parsing for Compressed Payloads

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/message/request.rs`

Add decompression logic to `create_frame_stream`:

```rust
// Add constant for compressed flag
const FLAG_COMPRESSED: u8 = 0x01;
const FLAG_END_STREAM: u8 = 0x02;

fn create_frame_stream<T>(
    body: Body,
    use_proto: bool,
    limits: MessageLimits,
    compression_ctx: Option<StreamingCompressionContext>,
) -> impl Stream<Item = Result<T, ConnectError>> + Send
where
    T: Message + DeserializeOwned + Default + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = BytesMut::new();
        let mut body = body;

        loop {
            while buffer.len() >= 5 {
                let flags = buffer[0];
                let length = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]) as usize;

                // Check message size limit BEFORE allocating
                if let Err(err) = limits.check_size(length) {
                    yield Err(ConnectError::new(Code::ResourceExhausted, err));
                    return;
                }

                if buffer.len() < 5 + length {
                    break; // Need more data
                }

                // EndStream frame
                if flags & FLAG_END_STREAM != 0 {
                    return;
                }

                // Extract payload
                let mut payload = buffer.split_to(5 + length).split_off(5);
                let payload_bytes = payload.freeze();

                // Handle compression flag
                let is_compressed = flags & FLAG_COMPRESSED != 0;
                let decompressed = if is_compressed {
                    match &compression_ctx {
                        Some(ctx) => match ctx.request_encoding {
                            Some(encoding) => {
                                match crate::compression::decompress(payload_bytes, encoding) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        yield Err(ConnectError::new(
                                            Code::InvalidArgument,
                                            format!("decompression failed: {}", e),
                                        ));
                                        return;
                                    }
                                }
                            }
                            None => {
                                yield Err(ConnectError::new(
                                    Code::InvalidArgument,
                                    "received compressed frame but no Connect-Content-Encoding header",
                                ));
                                return;
                            }
                        },
                        None => {
                            yield Err(ConnectError::new(
                                Code::Internal,
                                "compression context not available",
                            ));
                            return;
                        }
                    }
                } else {
                    payload_bytes
                };

                // Check decompressed size against limits
                if let Err(err) = limits.check_size(decompressed.len()) {
                    yield Err(ConnectError::new(Code::ResourceExhausted, err));
                    return;
                }

                // Decode the message
                let message = if use_proto {
                    T::decode(decompressed)
                        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))
                } else {
                    serde_json::from_slice(&decompressed)
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
```

### Step 4: Update Frame Writing for Compressed Responses

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/message/stream.rs`

Update `ConnectStreamResponse` to compress frame payloads:

```rust
const FLAG_COMPRESSED: u8 = 0x01;
const FLAG_END_STREAM: u8 = 0x02;

impl<S, T> IntoResponse for ConnectStreamResponse<S>
where
    S: Stream<Item = Result<T, ConnectError>> + Send + 'static,
    T: Message + Serialize + Send + 'static,
{
    fn into_response(self) -> Response {
        let protocol = self.protocol;
        let compression_ctx = self.compression_ctx; // Add field to struct
        let use_proto = protocol.is_proto();
        let content_type = protocol.streaming_response_content_type();

        let error_sent = Arc::new(AtomicBool::new(false));
        let error_sent_clone = error_sent.clone();

        let body_stream = self
            .stream
            .map(move |result| match result {
                Ok(msg) => {
                    // Encode message
                    let encoded = if use_proto {
                        msg.encode_to_vec()
                    } else {
                        match serde_json::to_vec(&msg) {
                            Ok(v) => v,
                            Err(_) => return (Bytes::from(internal_error_end_stream_frame()), true),
                        }
                    };

                    // Apply compression if configured and payload is large enough
                    let (payload, flags) = match &compression_ctx {
                        Some(ctx) if ctx.response_encoding != CompressionEncoding::Identity
                            && encoded.len() >= ctx.config.compress_min_bytes => {
                            match crate::compression::compress(&encoded, ctx.response_encoding) {
                                Ok(compressed) => (compressed, FLAG_COMPRESSED),
                                Err(_) => (encoded, 0x00), // Fallback to uncompressed
                            }
                        }
                        _ => (encoded, 0x00),
                    };

                    // Build frame: [flags][length BE][payload]
                    let len = payload.len() as u32;
                    let mut buf = Vec::with_capacity(5 + payload.len());
                    buf.push(flags);
                    buf.extend_from_slice(&len.to_be_bytes());
                    buf.extend(payload);

                    (Bytes::from(buf), false)
                }
                Err(err) => {
                    // Error EndStream frame (always uncompressed JSON)
                    let mut buf = vec![FLAG_END_STREAM, 0, 0, 0, 0];
                    let json = serde_json::json!({ "error": err });
                    match serde_json::to_writer(&mut buf, &json) {
                        Ok(()) => {
                            let len = (buf.len() - 5) as u32;
                            buf[1..5].copy_from_slice(&len.to_be_bytes());
                            (Bytes::from(buf), true)
                        }
                        Err(_) => (Bytes::from(internal_error_end_stream_frame()), true)
                    }
                }
            })
            // ... rest of stream handling (same as current)
    }
}
```

### Step 5: Update ConnectLayer for Streaming Compression

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/layer.rs`

Add streaming compression context extraction:

```rust
// In ConnectService::call, for streaming POST requests:
if *req.method() == Method::POST && protocol.is_streaming() {
    match streaming_compression::extract_streaming_compression_context(&req, &self.compression) {
        Ok(ctx) => {
            req.extensions_mut().insert(ctx);
        }
        Err(err) => {
            let response = err.into_response_with_protocol(protocol);
            return Box::pin(async move { Ok(response) });
        }
    }
}
```

### Step 6: Add Response Headers

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/message/stream.rs`

Add streaming compression headers to response:

```rust
// In IntoResponse implementation:
let mut builder = Response::builder()
    .status(StatusCode::OK)
    .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

// Add Connect-Content-Encoding if compressing
if let Some(ctx) = &compression_ctx {
    if ctx.response_encoding != CompressionEncoding::Identity {
        builder = builder.header(
            "connect-content-encoding",
            HeaderValue::from_static(ctx.response_encoding.as_str()),
        );
    }
}

// Always advertise accepted encodings
builder = builder.header(
    "connect-accept-encoding",
    HeaderValue::from_static(config.supported_encodings()),
);
```

### Step 7: Update Handler Wrappers

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/handler.rs`

Extract and pass streaming compression context to stream responses:

```rust
// In ConnectStreamHandlerWrapper, ConnectClientStreamHandlerWrapper, etc.:
let compression_ctx = req
    .extensions()
    .get::<StreamingCompressionContext>()
    .copied();

// Pass to response
let mut response = ConnectStreamResponse::new(stream);
response.compression_ctx = compression_ctx;
```

### Step 8: Update Module Exports

**File**: `/home/frank/github/connectrpc-axum/connectrpc-axum/src/lib.rs`

```rust
// In context/mod.rs:
mod streaming_compression;
pub use streaming_compression::StreamingCompressionContext;

// In layer/mod.rs:
mod streaming_compression;
pub(crate) use streaming_compression::extract_streaming_compression_context;
```

## Test Cases

### Unit Tests

1. **Header Parsing Tests**
   ```rust
   #[test]
   fn test_parse_connect_content_encoding() {
       // Valid: gzip, identity, empty
       // Invalid: br, deflate, unknown
   }

   #[test]
   fn test_negotiate_connect_accept_encoding() {
       // Returns gzip if supported and requested
       // Falls back to identity
   }
   ```

2. **Frame Flag Tests**
   ```rust
   #[test]
   fn test_compressed_frame_flag() {
       // Frame with flag 0x01 is decompressed
       // Frame with flag 0x00 is not decompressed
       // Frame with flag 0x03 (compressed + endstream) errors
   }
   ```

### Integration Tests

1. **Compressed Request Frames**
   ```rust
   #[tokio::test]
   async fn test_streaming_request_decompression() {
       // Send request with Connect-Content-Encoding: gzip
       // Send frames with flag 0x01 and gzip-compressed payloads
       // Verify handler receives decompressed messages
   }
   ```

2. **Compressed Response Frames**
   ```rust
   #[tokio::test]
   async fn test_streaming_response_compression() {
       // Send request with Connect-Accept-Encoding: gzip
       // Verify response has Connect-Content-Encoding: gzip
       // Verify response frames have flag 0x01 with compressed payloads
   }
   ```

3. **Mixed Compression**
   ```rust
   #[tokio::test]
   async fn test_mixed_compressed_uncompressed_frames() {
       // Server can mix compressed and uncompressed frames
       // Based on per-frame size threshold
   }
   ```

4. **Error Cases**
   ```rust
   #[tokio::test]
   async fn test_compressed_frame_without_header() {
       // Receive frame with flag 0x01 but no Connect-Content-Encoding header
       // Should return error
   }

   #[tokio::test]
   async fn test_unsupported_encoding_error() {
       // Send request with Connect-Content-Encoding: br
       // Should return Unimplemented error in response
   }
   ```

5. **Small Frames Not Compressed**
   ```rust
   #[tokio::test]
   async fn test_small_frames_not_compressed() {
       // Frames below compress_min_bytes threshold
       // Should have flag 0x00 (not compressed)
   }
   ```

## Edge Cases

1. **Missing Connect-Content-Encoding header but compressed frame received**
   - Return `Code::InvalidArgument` error
   - Message: "received compressed frame but no Connect-Content-Encoding header"

2. **Decompression failure** (corrupted data)
   - Return `Code::InvalidArgument` with decompression error message

3. **Compression failure** on response
   - Fall back to uncompressed frame (flag 0x00)
   - Don't fail the stream

4. **EndStream frames**
   - Always sent as JSON, never compressed (per protocol spec)
   - Flag 0x02, never 0x03

5. **Empty frames**
   - Valid: frame with length 0
   - Don't attempt compression on empty payloads

6. **Size limits after decompression**
   - Check `MessageLimits` against decompressed size, not compressed size
   - This prevents decompression bombs

## File Summary

| File | Action | Description |
|------|--------|-------------|
| `context/streaming_compression.rs` | Create | StreamingCompressionContext type |
| `context/mod.rs` | Modify | Export StreamingCompressionContext |
| `layer/streaming_compression.rs` | Create | Header parsing and validation |
| `layer/mod.rs` | Modify | Export streaming compression functions |
| `layer.rs` | Modify | Extract streaming compression context |
| `message/request.rs` | Modify | Decompress frame payloads |
| `message/stream.rs` | Modify | Compress frame payloads, add response headers |
| `handler.rs` | Modify | Pass compression context to responses |
| `lib.rs` | Modify | Re-export types if needed |

## Implementation Order

1. Create `StreamingCompressionContext` type
2. Create streaming header parsing (`layer/streaming_compression.rs`)
3. Update `ConnectLayer` to extract context for streaming requests
4. Update frame parsing to handle compressed flag and decompress
5. Update `ConnectStreamResponse` to compress frames and add headers
6. Update handler wrappers to pass compression context
7. Add unit tests
8. Add integration tests with connect-go client

## Compatibility Notes

- This implementation is for **streaming RPCs only**
- Unary RPCs use different headers (PR4)
- EndStream frames are never compressed
- The implementation matches connect-go behavior for header parsing and error codes
- Per-frame compression decision (based on size threshold) matches connect-go
