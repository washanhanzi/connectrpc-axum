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

Reuse and extend types from `connectrpc-axum/src/message/response.rs`:

```rust
use http::HeaderMap;

/// Metadata from response headers/trailers
#[derive(Debug, Default, Clone)]
pub struct Metadata {
    headers: HeaderMap,
}

impl Metadata {
    pub fn headers(&self) -> &HeaderMap { &self.headers }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(|v| v.to_str().ok())
    }
}

/// Response wrapper providing access to message and metadata
pub struct ConnectResponse<T> {
    inner: T,
    metadata: Metadata,
}

impl<T> ConnectResponse<T> {
    pub fn new(inner: T) -> Self {
        Self { inner, metadata: Metadata::default() }
    }

    pub fn with_metadata(inner: T, metadata: Metadata) -> Self {
        Self { inner, metadata }
    }

    pub fn into_inner(self) -> T { self.inner }
    pub fn metadata(&self) -> &Metadata { &self.metadata }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ConnectResponse<U> {
        ConnectResponse { inner: f(self.inner), metadata: self.metadata }
    }
}

impl<T> std::ops::Deref for ConnectResponse<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.inner }
}

/// Wrapper for streaming responses with trailers access
pub struct StreamBody<S> {
    decoder: S,
}

impl<S> StreamBody<S> {
    pub fn new(decoder: S) -> Self { Self { decoder } }
    pub fn into_inner(self) -> S { self.decoder }
}

impl<S, T> StreamBody<FrameDecoder<S, T>> {
    /// Get trailers after stream is fully consumed.
    /// Returns None if stream hasn't finished or ended with error.
    pub fn trailers(&self) -> Option<&Metadata> {
        self.decoder.trailers()
    }
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

flags (bitmask):
  - 0b0000_0001 (0x01): compressed payload
  - 0b0000_0010 (0x02): end-of-stream (trailers/error)

Examples:
  - 0x00: normal message, uncompressed
  - 0x01: normal message, compressed
  - 0x02: end-of-stream, uncompressed payload
  - 0x03: end-of-stream, compressed payload
```

### FrameDecoder

**Design principle**: Errors are yielded as `Some(Err(e))` in the stream, not stored in side-channels.
This follows Rust idioms where consumers expect errors in the stream itself.

```rust
pub struct FrameDecoder<S, T> {
    stream: S,
    buffer: BytesMut,
    use_proto: bool,
    compression: CompressionEncoding,
    trailers: Option<Metadata>,  // Successful end-stream metadata (not errors)
    finished: bool,
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
        // 3. Check flags (bitmask):
        //    - If compressed (0x01): decompress payload
        //    - If end-stream (0x02): parse EndStream JSON
        //      - If error present: return Some(Err(connect_error))
        //      - If success: store trailers, return None
        //    - Otherwise: decode message, return Some(Ok(message))
        // 4. Handle partial frames across chunk boundaries (loop until complete)
        // 5. On unexpected EOF: return Some(Err(Code::DataLoss, "stream closed unexpectedly"))
    }
}

impl<S, T> FrameDecoder<S, T> {
    /// Get response trailers/metadata after stream completes successfully.
    /// Returns None if stream hasn't finished or ended with error.
    pub fn trailers(&self) -> Option<&Metadata> {
        self.trailers.as_ref()
    }
}
```

### FrameEncoder

```rust
pub struct FrameEncoder<S, T> {
    stream: S,
    use_proto: bool,
    compression: CompressionConfig,
    end_stream_sent: bool,
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
        // 2. If message available:
        //    a. Encode message (proto or JSON)
        //    b. Optionally compress (set 0x01 flag)
        //    c. Wrap in envelope frame
        //    d. Return frame bytes
        // 3. If inner stream exhausted and !end_stream_sent:
        //    a. Build EndStream frame (flag 0x02, empty or with metadata)
        //    b. Set end_stream_sent = true
        //    c. Return EndStream frame
        // 4. If end_stream_sent: return None
    }
}

impl<S, T> Drop for FrameEncoder<S, T> {
    fn drop(&mut self) {
        // Note: Cannot send EndStream in Drop (async context not available).
        // Callers should ensure stream is fully consumed or use explicit
        // finish() method before dropping for clean shutdown.
        if !self.end_stream_sent {
            tracing::debug!("FrameEncoder dropped without sending EndStream");
        }
    }
}

impl<S, T> FrameEncoder<S, T> {
    /// Explicitly finish the stream, returning the EndStream frame.
    /// Call this for clean shutdown if not consuming all items.
    pub fn finish(&mut self) -> Option<Bytes> {
        if self.end_stream_sent {
            return None;
        }
        self.end_stream_sent = true;
        Some(wrap_envelope(&[], FLAG_END_STREAM))
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

### Phase 5: Code Generation

Add `client` feature to `connectrpc-axum-build` that generates typed client structs.

#### Feature Flag & Dependencies

```toml
# connectrpc-axum-build/Cargo.toml
[dependencies]
# ... existing deps ...
connectrpc-axum-client = { path = "../connectrpc-axum-client", optional = true }

[features]
default = []
client = ["dep:connectrpc-axum-client"]  # Enables Connect client code generation
```

The `client` feature brings in `connectrpc-axum-client` as a dependency, which provides:
- `ConnectClient` - the underlying HTTP client
- `ClientBuilder` - client configuration builder
- `ConnectResponse<T>` - response wrapper type
- `StreamBody<S>` - streaming response wrapper
- Frame encoding/decoding utilities

#### Builder API

```rust
// Type-state marker
pub struct ClientEnabled;
pub struct ClientDisabled;

impl<C, T, TC> CompileBuilder<C, T, TC, ClientDisabled> {
    /// Enable Connect client code generation
    ///
    /// Generates typed client structs like `HelloWorldServiceClient` with
    /// methods for each RPC. Requires the `client` feature.
    #[cfg(feature = "client")]
    pub fn with_client(self) -> CompileBuilder<C, T, TC, ClientEnabled> { ... }
}
```

#### Generated Client Struct

For a service like:

```protobuf
service HelloWorldService {
  rpc SayHello(HelloRequest) returns (HelloResponse);
  rpc SayHelloStream(HelloRequest) returns (stream HelloResponse);
  rpc SayHelloClientStream(stream HelloRequest) returns (HelloResponse);
  rpc SayHelloBidi(stream HelloRequest) returns (stream HelloResponse);
}
```

Generate:

```rust
/// Generated Connect client for HelloWorldService
pub struct HelloWorldServiceClient {
    client: connectrpc_axum_client::ConnectClient,
}

impl HelloWorldServiceClient {
    /// Create a new client with the given base URL
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: connectrpc_axum_client::ConnectClient::builder(base_url).build(),
        }
    }

    /// Create from an existing ConnectClient
    pub fn from_client(client: connectrpc_axum_client::ConnectClient) -> Self {
        Self { client }
    }

    /// Get builder for custom configuration
    pub fn builder(base_url: impl Into<String>) -> HelloWorldServiceClientBuilder {
        HelloWorldServiceClientBuilder::new(base_url)
    }

    /// Unary RPC: SayHello
    pub async fn say_hello(
        &self,
        request: HelloRequest,
    ) -> Result<connectrpc_axum_client::ConnectResponse<HelloResponse>, connectrpc_axum_core::ConnectError> {
        self.client
            .call_unary("hello.v1.HelloWorldService/SayHello", &request)
            .await
    }

    /// Server streaming RPC: SayHelloStream
    pub async fn say_hello_stream(
        &self,
        request: HelloRequest,
    ) -> Result<
        connectrpc_axum_client::ConnectResponse<
            connectrpc_axum_client::StreamBody<
                impl futures::Stream<Item = Result<HelloResponse, connectrpc_axum_core::ConnectError>>
            >
        >,
        connectrpc_axum_core::ConnectError,
    > {
        self.client
            .call_server_stream("hello.v1.HelloWorldService/SayHelloStream", &request)
            .await
    }

    /// Client streaming RPC: SayHelloClientStream
    pub async fn say_hello_client_stream(
        &self,
        request: impl futures::Stream<Item = HelloRequest> + Send + 'static,
    ) -> Result<connectrpc_axum_client::ConnectResponse<HelloResponse>, connectrpc_axum_core::ConnectError> {
        self.client
            .call_client_stream("hello.v1.HelloWorldService/SayHelloClientStream", request)
            .await
    }

    /// Bidi streaming RPC: SayHelloBidi
    pub async fn say_hello_bidi(
        &self,
        request: impl futures::Stream<Item = HelloRequest> + Send + 'static,
    ) -> Result<
        connectrpc_axum_client::ConnectResponse<
            connectrpc_axum_client::StreamBody<
                impl futures::Stream<Item = Result<HelloResponse, connectrpc_axum_core::ConnectError>>
            >
        >,
        connectrpc_axum_core::ConnectError,
    > {
        self.client
            .call_bidi_stream("hello.v1.HelloWorldService/SayHelloBidi", request)
            .await
    }
}
```

#### Generated Client Builder

```rust
/// Builder for HelloWorldServiceClient with custom configuration
pub struct HelloWorldServiceClientBuilder {
    builder: connectrpc_axum_client::ClientBuilder,
}

impl HelloWorldServiceClientBuilder {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            builder: connectrpc_axum_client::ConnectClient::builder(base_url),
        }
    }

    /// Use an existing reqwest::Client
    pub fn client(mut self, client: reqwest::Client) -> Self {
        self.builder = self.builder.client(client);
        self
    }

    /// Add middleware
    pub fn with_middleware<M: reqwest_middleware::Middleware + 'static>(mut self, middleware: M) -> Self {
        self.builder = self.builder.with_middleware(middleware);
        self
    }

    /// Use JSON encoding instead of protobuf
    pub fn use_json(mut self) -> Self {
        self.builder = self.builder.use_json();
        self
    }

    /// Configure compression
    pub fn compression(mut self, config: connectrpc_axum_core::CompressionConfig) -> Self {
        self.builder = self.builder.compression(config);
        self
    }

    /// Build the client
    pub fn build(self) -> HelloWorldServiceClient {
        HelloWorldServiceClient {
            client: self.builder.build(),
        }
    }
}
```

#### Usage Example

```rust
use hello::v1::{HelloWorldServiceClient, HelloRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Simple usage
    let client = HelloWorldServiceClient::new("http://localhost:3000");
    let response = client.say_hello(HelloRequest { name: "World".into() }).await?;
    println!("Response: {:?}", response.into_inner());

    // With custom configuration
    let client = HelloWorldServiceClient::builder("http://localhost:3000")
        .use_json()
        .with_middleware(reqwest_tracing::TracingMiddleware::default())
        .build();

    // Server streaming
    let stream_response = client.say_hello_stream(HelloRequest { name: "World".into() }).await?;
    let mut stream = stream_response.into_inner().into_inner();
    while let Some(msg) = stream.next().await {
        println!("Streamed: {:?}", msg?);
    }

    Ok(())
}
```

#### Implementation Tasks

- [ ] Add `client` feature to `connectrpc-axum-build/Cargo.toml` with `connectrpc-axum-client` dependency
- [ ] Add `ClientEnabled`/`ClientDisabled` type markers
- [ ] Add `with_client()` method to `CompileBuilder` (gated by `#[cfg(feature = "client")]`)
- [ ] Create `ConnectClientServiceGenerator` implementing `ServiceGenerator`
- [ ] Generate client struct with `new()`, `from_client()`, `builder()` methods
- [ ] Generate typed RPC methods for each service method:
  - Unary: `async fn method(&self, request: Req) -> Result<ConnectResponse<Res>, ConnectError>`
  - Server stream: Returns `StreamBody<impl Stream<...>>`
  - Client stream: Takes `impl Stream<Item = Req>`
  - Bidi: Takes stream, returns stream
- [ ] Generate client builder struct with configuration methods
- [ ] Add integration tests with real server/client roundtrip

## Dependencies

### connectrpc-axum-core

```toml
[dependencies]
bytes = { workspace = true }
http = { workspace = true }
prost = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = "1.0"
flate2 = { workspace = true }
# Note: No tower-http dependency - define our own CompressionLevel to keep core lightweight

brotli = { workspace = true, optional = true }
zstd = { workspace = true, optional = true }

[features]
default = []
compression-deflate = []
compression-br = ["dep:brotli"]
compression-zstd = ["dep:zstd"]
compression-full = ["compression-deflate", "compression-br", "compression-zstd"]
```

#### Error Model (in connectrpc-axum-core)

```rust
use thiserror::Error;

/// Connect protocol status code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Canceled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

/// Structured error type for Connect protocol errors
#[derive(Debug, Error)]
pub enum ConnectError {
    /// Protocol-level error returned by server (has Code + message + details)
    #[error("connect error {code:?}: {message}")]
    Status {
        code: Code,
        message: String,
        details: Vec<ErrorDetail>,
    },

    /// HTTP transport error (connection failed, timeout, etc.)
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Failed to encode request message
    #[error("encode error: {0}")]
    Encode(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Failed to decode response message
    #[error("decode error: {0}")]
    Decode(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Protocol violation (invalid frame, unexpected EOF, etc.)
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl ConnectError {
    /// Create a Status error with the given code and message
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self::Status {
            code,
            message: message.into(),
            details: vec![],
        }
    }

    /// Get the error code if this is a Status error
    pub fn code(&self) -> Option<Code> {
        match self {
            Self::Status { code, .. } => Some(*code),
            _ => None,
        }
    }
}
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

## Review Findings (Addressed)

Based on reviews from Gemini and Codex, the following improvements were incorporated:

| Finding | Resolution |
|---------|------------|
| **Error propagation in streams** | FrameDecoder now yields errors as `Some(Err(e))` in the stream, not via side-channel methods. Only successful trailers are stored for post-stream access. |
| **Frame flags as bitmask** | Flags are now documented as bitmask values (0x01=compressed, 0x02=end-stream) supporting combinations like 0x03 for compressed end-stream. |
| **Cancellation/drop handling** | FrameEncoder includes `end_stream_sent` tracking, Drop impl with warning, and explicit `finish()` method for clean shutdown. |
| **Heavy tower-http dependency** | Removed `tower-http` from core crate dependencies. Core defines its own `CompressionLevel` enum instead of re-exporting. |
| **Structured error model** | Added `ConnectError` enum with clear variants: `Status`, `Transport`, `Encode`, `Decode`, `Protocol` using `thiserror`. |
| **Trailers/metadata access** | Added `Metadata` struct and `trailers()` method on `StreamBody<FrameDecoder<S, T>>`. `ConnectResponse<T>` includes `metadata()` accessor. |
| **Response ergonomics** | `ConnectResponse<T>` implements `Deref<Target=T>` for convenient access. |
| **Type-state breaking change** | `ClientDisabled` is the default, ensuring existing build scripts continue to work. |
