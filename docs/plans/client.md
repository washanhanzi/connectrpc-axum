# ConnectRPC Client for Rust

A ConnectRPC client subcrate built on `reqwest` with `reqwest-middleware` support.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│              ConnectRPC Client API                      │
│    client.call_unary() / call_server_stream() / ...     │
│    Returns: ConnectResponse<T> or                       │
│             ConnectResponse<StreamBody<Stream<T>>>      │
├─────────────────────────────────────────────────────────┤
│         Protocol Layer (Frame Codec)                    │
│    FrameEncoder → request frames                        │
│    FrameDecoder ← response frames                       │
│    (operates on Stream<Bytes>, NOT middleware)          │
├─────────────────────────────────────────────────────────┤
│         reqwest-middleware                              │
│    Auth, Retry, Tracing, Timeout (request-level)        │
├─────────────────────────────────────────────────────────┤
│              reqwest::Client                            │
├─────────────────────────────────────────────────────────┤
│                     hyper                               │
└─────────────────────────────────────────────────────────┘
```

## Key Design Decisions

1. **Unified Response Types** - Reuse `ConnectResponse<T>` and `StreamBody<S>` from server
2. **Filename-based Modules** - No `mod.rs` files; use `file.rs` pattern
3. **Shared Core Crate** - Extract protocol types to `connectrpc-axum-core`

## Architecture: Core Crate Extraction

Extract ~1,200 lines of protocol-agnostic code into `connectrpc-axum-core`:

| Component | Source Location | Purpose |
|-----------|-----------------|---------|
| `Code`, `ConnectError`, `ErrorDetail` | `error.rs` | Error model |
| `Codec` trait, `GzipCodec`, etc. | `context/envelope_compression.rs` | Compression |
| `CompressionEncoding`, `CompressionConfig` | `context/envelope_compression.rs` | Encoding config |
| `wrap_envelope`, `process_envelope_payload` | `pipeline.rs:130-242` | Frame codec |
| `envelope_flags` (0x00, 0x01, 0x02) | `pipeline.rs:214-221` | Protocol constants |

## Crate Structure

```
connectrpc-axum/
├── connectrpc-axum-core/          # Shared protocol types
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                 # Re-exports all public types
│       ├── error.rs               # Code, ConnectError, ErrorDetail
│       ├── codec.rs               # Codec trait, BoxedCodec, GzipCodec, etc.
│       ├── compression.rs         # CompressionConfig, CompressionEncoding
│       └── envelope.rs            # envelope_flags, wrap_envelope, process_envelope_payload
│
├── connectrpc-axum-client/        # Client implementation
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                 # Re-exports ConnectClient, response types
│       ├── client.rs              # ConnectClient struct and builder
│       ├── call.rs                # call_unary, call_server_stream, etc.
│       ├── frame.rs               # FrameDecoder, FrameEncoder
│       └── streaming.rs           # Client-side streaming utilities
│
├── connectrpc-axum/               # Server (updated to use core)
```

## Why This Architecture

### Why reqwest (not raw hyper)?

- reqwest already handles: connection pooling, TLS, HTTP/2 negotiation, cookies, redirects
- Built on hyper anyway - no performance loss
- Simpler API for common cases
- Can still access hyper internals if needed

### Why reqwest-middleware?

- Provides middleware chain composition for request-level concerns
- Ecosystem includes: retry, tracing, caching middleware
- Mirrors reqwest API - easy to adopt

### Limitation: reqwest-middleware and Streaming

The `Middleware` trait operates at **request/response level**, NOT chunk level:

```rust
pub trait Middleware: 'static + Send + Sync {
    async fn handle(
        &self,
        req: Request,           // Complete request (body is opaque)
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response>;      // Complete response (body stream passes through)
}
```

| Concern | reqwest-middleware | Works? |
|---------|-------------------|--------|
| Add auth headers | ✅ Modify `req.headers()` | Yes |
| Timeout/retry | ✅ Wrap `next.run()` | Yes |
| Tracing/logging | ✅ Instrument around call | Yes |
| **Encode streaming body frames** | ❌ Body is opaque | No |
| **Decode streaming response frames** | ❌ Body passes through | No |

**Solution**: Use reqwest-middleware for HTTP concerns, build frame codec as separate layer on body streams.

## Unified Response Types

Reuse existing types from `connectrpc-axum/src/message/response.rs`:

```rust
// Simple wrapper for any response
pub struct ConnectResponse<T>(pub T);

impl<T> ConnectResponse<T> {
    pub fn new(inner: T) -> Self { Self(inner) }
    pub fn into_inner(self) -> T { self.0 }
}

// Wrapper for streaming responses
pub struct StreamBody<S> {
    stream: S,
}

impl<S> StreamBody<S> {
    pub fn new(stream: S) -> Self { Self { stream } }
    pub fn into_inner(self) -> S { self.stream }
}
```

## Core Types

### ConnectClient

```rust
use reqwest_middleware::ClientWithMiddleware;

pub struct ConnectClient {
    http: ClientWithMiddleware,
    base_url: String,
    use_proto: bool,  // true = protobuf, false = JSON
    compression: CompressionConfig,
    accept_encoding: Option<CompressionEncoding>,
}

impl ConnectClient {
    pub fn builder(base_url: impl Into<String>) -> ClientBuilder { ... }
}
```

### ClientBuilder

```rust
pub struct ClientBuilder {
    base_url: String,
    client: Option<reqwest::Client>,
    middlewares: Vec<Arc<dyn Middleware>>,
    use_proto: bool,
    compression: CompressionConfig,
    accept_encoding: Option<CompressionEncoding>,
}

impl ClientBuilder {
    pub fn new(base_url: impl Into<String>) -> Self;
    pub fn client(self, client: reqwest::Client) -> Self;
    pub fn with_middleware<M: Middleware + 'static>(self, middleware: M) -> Self;
    pub fn use_json(self) -> Self;
    pub fn use_proto(self) -> Self;  // default
    pub fn compression(self, config: CompressionConfig) -> Self;
    pub fn accept_encoding(self, encoding: CompressionEncoding) -> Self;
    pub fn build(self) -> ConnectClient;
}
```

## RPC Method Implementations

### Unary Call

```rust
impl ConnectClient {
    pub async fn call_unary<Req, Res>(
        &self,
        procedure: &str,
        request: &Req,
    ) -> Result<ConnectResponse<Res>, ConnectError>
    where
        Req: prost::Message + serde::Serialize,
        Res: prost::Message + serde::de::DeserializeOwned + Default,
    {
        let body = self.encode_message(request)?;

        let response = self.http
            .post(format!("{}/{}", self.base_url, procedure))
            .header("content-type", self.unary_content_type())
            .header("connect-protocol-version", "1")
            .body(body)
            .send()
            .await?;

        // Check for Connect error in headers/body
        if !response.status().is_success() {
            return Err(self.parse_error_response(response).await);
        }

        let bytes = response.bytes().await?;
        let message = self.decode_message(&bytes)?;
        Ok(ConnectResponse::new(message))
    }
}
```

### Server Streaming

```rust
impl ConnectClient {
    pub async fn call_server_stream<Req, Res>(
        &self,
        procedure: &str,
        request: &Req,
    ) -> Result<ConnectResponse<StreamBody<impl Stream<Item = Result<Res, ConnectError>>>>, ConnectError>
    where
        Req: prost::Message + serde::Serialize,
        Res: prost::Message + serde::de::DeserializeOwned + Default + Send + 'static,
    {
        // Encode request as envelope frame
        let body = wrap_envelope(&self.encode_message(request)?, false);

        let response = self.http
            .post(format!("{}/{}", self.base_url, procedure))
            .header("content-type", self.streaming_content_type())
            .header("connect-protocol-version", "1")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.parse_error_response(response).await);
        }

        // Create FrameDecoder wrapping response stream
        let byte_stream = response.bytes_stream();
        let compression = self.parse_response_compression(&response);
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, compression);

        Ok(ConnectResponse::new(StreamBody::new(decoder)))
    }
}
```

### Client Streaming

```rust
impl ConnectClient {
    pub async fn call_client_stream<Req, Res>(
        &self,
        procedure: &str,
        request: impl Stream<Item = Req> + Send + 'static,
    ) -> Result<ConnectResponse<Res>, ConnectError>
    where
        Req: prost::Message + serde::Serialize + Send + 'static,
        Res: prost::Message + serde::de::DeserializeOwned + Default,
    {
        // Encode stream as envelope frames
        let frame_stream = FrameEncoder::new(request, self.use_proto, self.compression.clone());
        let body = reqwest::Body::wrap_stream(frame_stream);

        let response = self.http
            .post(format!("{}/{}", self.base_url, procedure))
            .header("content-type", self.streaming_content_type())
            .header("connect-protocol-version", "1")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.parse_error_response(response).await);
        }

        // Response is streaming format (single message + EndStream)
        let mut decoder = FrameDecoder::new(
            response.bytes_stream(),
            self.use_proto,
            self.parse_response_compression(&response),
        );

        // Read the single response message
        match decoder.next().await {
            Some(Ok(message)) => Ok(ConnectResponse::new(message)),
            Some(Err(e)) => Err(e),
            None => Err(ConnectError::new(Code::Internal, "empty response")),
        }
    }
}
```

### Bidi Streaming (HTTP/2 only)

```rust
impl ConnectClient {
    pub async fn call_bidi_stream<Req, Res>(
        &self,
        procedure: &str,
        request: impl Stream<Item = Req> + Send + 'static,
    ) -> Result<ConnectResponse<StreamBody<impl Stream<Item = Result<Res, ConnectError>>>>, ConnectError>
    where
        Req: prost::Message + serde::Serialize + Send + 'static,
        Res: prost::Message + serde::de::DeserializeOwned + Default + Send + 'static,
    {
        // Encode stream as envelope frames
        let frame_stream = FrameEncoder::new(request, self.use_proto, self.compression.clone());
        let body = reqwest::Body::wrap_stream(frame_stream);

        let response = self.http
            .post(format!("{}/{}", self.base_url, procedure))
            .header("content-type", self.streaming_content_type())
            .header("connect-protocol-version", "1")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.parse_error_response(response).await);
        }

        // Create FrameDecoder for response stream
        let decoder = FrameDecoder::new(
            response.bytes_stream(),
            self.use_proto,
            self.parse_response_compression(&response),
        );

        Ok(ConnectResponse::new(StreamBody::new(decoder)))
    }
}
```

## Frame Codec

### Envelope Format

```
[flags: 1 byte][length: 4 bytes BE][payload: length bytes]

flags:
  - 0x00: normal message (uncompressed)
  - 0x01: compressed message
  - 0x02: end-of-stream (trailers/error)
```

### FrameDecoder

```rust
pub struct FrameDecoder<S, T> {
    stream: S,
    buffer: BytesMut,
    use_proto: bool,
    compression: CompressionEncoding,
    end_stream: Option<EndStreamPayload>,
    _marker: PhantomData<T>,
}

impl<S, T> Stream for FrameDecoder<S, T>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
    T: prost::Message + serde::de::DeserializeOwned + Default,
{
    type Item = Result<T, ConnectError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 1. Buffer bytes until we have at least 5 (header)
        // 2. Parse length, buffer until complete frame
        // 3. Check flags:
        //    - 0x00/0x01: decode message, handle compression
        //    - 0x02: parse EndStream, check for error, return None
        // 4. Return decoded message
    }
}

impl<S, T> FrameDecoder<S, T> {
    /// Get EndStream payload after stream completes (if any)
    pub fn end_stream(&self) -> Option<&EndStreamPayload> {
        self.end_stream.as_ref()
    }

    /// Check if stream ended with error
    pub fn error(&self) -> Option<&ConnectError> {
        self.end_stream.as_ref().and_then(|e| e.error.as_ref())
    }
}
```

### FrameEncoder

```rust
pub struct FrameEncoder<S, T> {
    stream: S,
    use_proto: bool,
    compression: CompressionConfig,
    _marker: PhantomData<T>,
}

impl<S, T> Stream for FrameEncoder<S, T>
where
    S: Stream<Item = T> + Unpin,
    T: prost::Message + serde::Serialize,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 1. Poll inner stream for next message
        // 2. Encode message (proto or JSON)
        // 3. Optionally compress
        // 4. Wrap in envelope frame
        // 5. Return frame bytes
    }
}
```

## Implementation Phases

### Phase 0: Core Crate Extraction
- [ ] Create `connectrpc-axum-core` crate
- [ ] Extract `Code`, `ConnectError`, `ErrorDetail` from error.rs
- [ ] Extract `Codec` trait and compression impls
- [ ] Extract `wrap_envelope`, `process_envelope_payload`, flags
- [ ] Update `connectrpc-axum` to depend on and re-export from core
- [ ] Ensure all existing tests pass

### Phase 1: Unary Calls
- [ ] Create `connectrpc-axum-client` crate scaffold
- [ ] Implement `ConnectClient` and `ClientBuilder`
- [ ] Implement `call_unary()` with proto/json encoding
- [ ] Parse error responses (JSON body or HTTP status)
- [ ] Basic compression support

### Phase 2: Server Streaming
- [ ] Implement `FrameDecoder` for response streams
- [ ] Handle EndStream frames (flag 0x02) for trailers/errors
- [ ] Per-frame decompression
- [ ] Implement `call_server_stream()`

### Phase 3: Client Streaming
- [ ] Implement `FrameEncoder` for request streams
- [ ] `Body::wrap_stream()` integration
- [ ] Per-frame compression
- [ ] Implement `call_client_stream()`

### Phase 4: Bidi Streaming
- [ ] Implement `call_bidi_stream()`
- [ ] HTTP/2 requirement validation

### Phase 5: Code Generation (Optional)
- [ ] Update `connectrpc-axum-build` with `with_connect_client()` method
- [ ] Generate typed client traits per service

## Dependencies

### connectrpc-axum-core

```toml
[dependencies]
bytes = { workspace = true }
prost = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = "1.0"
flate2 = { workspace = true }
tower-http = { workspace = true }  # CompressionLevel re-export

brotli = { workspace = true, optional = true }
zstd = { workspace = true, optional = true }

[features]
default = []
compression-deflate = []
compression-br = ["dep:brotli"]
compression-zstd = ["dep:zstd"]
compression-full = ["compression-deflate", "compression-br", "compression-zstd"]
```

### connectrpc-axum-client

```toml
[dependencies]
connectrpc-axum-core = { path = "../connectrpc-axum-core" }
bytes = { workspace = true }
futures = { workspace = true }
http = { workspace = true }
prost = { workspace = true }
reqwest = { version = "0.12", features = ["stream", "http2"] }
reqwest-middleware = "0.4"
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true, features = ["sync"] }
tokio-stream = { workspace = true }
pin-project-lite = "0.2"
thiserror = "1.0"

[features]
default = []
compression-deflate = ["connectrpc-axum-core/compression-deflate"]
compression-br = ["connectrpc-axum-core/compression-br"]
compression-zstd = ["connectrpc-axum-core/compression-zstd"]
compression-full = ["connectrpc-axum-core/compression-full"]
```

## References

- [connect-go source](https://github.com/connectrpc/connect-go) - Official Go implementation
- [Connect Protocol Spec](https://connectrpc.com/docs/protocol)
- [reqwest-middleware](https://github.com/TrueLayer/reqwest-middleware)
