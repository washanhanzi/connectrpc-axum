# ConnectRPC Client Implementation Steps

Step-by-step implementation guide for the ConnectRPC client. Each task includes acceptance criteria and dependencies.

## Prerequisites

Before starting, ensure you understand:
- The existing `connectrpc-axum` server codebase
- Connect protocol specification (envelope format, error handling)
- `reqwest` and `reqwest-middleware` APIs

---

## Phase 0: Core Crate Extraction

**Goal**: Extract ~1,200 lines of shared protocol code into `connectrpc-axum-core`.

### Step 0.1: Create core crate scaffold

```bash
mkdir -p connectrpc-axum-core/src
```

**Files to create**:
- [x] `connectrpc-axum-core/Cargo.toml`
- [x] `connectrpc-axum-core/src/lib.rs`

**Cargo.toml contents**:
```toml
[package]
name = "connectrpc-axum-core"
version = "0.1.0"
edition = "2021"

[dependencies]
bytes = { workspace = true }
http = { workspace = true }
prost = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = "1.0"
flate2 = { workspace = true }

brotli = { workspace = true, optional = true }
zstd = { workspace = true, optional = true }

[features]
default = []
compression-br = ["dep:brotli"]
compression-zstd = ["dep:zstd"]
compression-full = ["compression-br", "compression-zstd"]
```

**Acceptance**: `cargo check -p connectrpc-axum-core` passes.

---

### Step 0.2: Extract error types

**Source**: `connectrpc-axum/src/error.rs`

**Create**: `connectrpc-axum-core/src/error.rs`

**Types to move**:
- [x] `Code` enum (protocol status codes)
- [x] `ConnectError` enum (with variants: `Status`, `Transport`, `Encode`, `Decode`, `Protocol`)
- [x] `ErrorDetail` struct

**Acceptance**: Error types compile with `thiserror` derives.

---

### Step 0.3: Extract compression types

**Source**: `connectrpc-axum/src/context/envelope_compression.rs`

**Create**: `connectrpc-axum-core/src/compression.rs`

**Types to move**:
- [x] `CompressionEncoding` enum
- [x] `CompressionConfig` struct
- [x] `CompressionLevel` enum (define locally, don't import from tower-http)

**Acceptance**: Compression config types compile independently.

---

### Step 0.4: Extract codec trait and implementations

**Source**: `connectrpc-axum/src/context/envelope_compression.rs`

**Create**: `connectrpc-axum-core/src/codec.rs`

**Types to move**:
- [x] `Codec` trait
- [x] `BoxedCodec` type alias
- [x] `GzipCodec` implementation
- [x] `DeflateCodec` implementation
- [x] `BrotliCodec` implementation (feature-gated)
- [x] `ZstdCodec` implementation (feature-gated)
- [x] `IdentityCodec` implementation

**Acceptance**: All codec implementations compile with feature gates.

---

### Step 0.5: Extract envelope functions

**Source**: `connectrpc-axum/src/pipeline.rs` (lines ~130-242)

**Create**: `connectrpc-axum-core/src/envelope.rs`

**Functions/constants to move**:
- [x] `FLAG_COMPRESSED` (0x01)
- [x] `FLAG_END_STREAM` (0x02)
- [x] `wrap_envelope(payload, flags) -> Bytes`
- [x] `process_envelope_payload(data, codec) -> Result<Bytes>`
- [x] `parse_envelope_header(data) -> Result<(flags, length)>`

**Acceptance**: Envelope functions work with bitmask flags.

---

### Step 0.6: Create metadata type

**Create**: `connectrpc-axum-core/src/metadata.rs`

**Types**:
- [x] `Metadata` struct wrapping `http::HeaderMap`
- [x] `get()`, `headers()` accessors

**Acceptance**: Metadata type compiles.

---

### Step 0.7: Wire up lib.rs re-exports

**Update**: `connectrpc-axum-core/src/lib.rs`

```rust
mod codec;
mod compression;
mod envelope;
mod error;
mod metadata;

pub use codec::*;
pub use compression::*;
pub use envelope::*;
pub use error::*;
pub use metadata::*;
```

**Acceptance**: `cargo doc -p connectrpc-axum-core` shows all public types.

---

### Step 0.8: Update connectrpc-axum to use core

**Update**: `connectrpc-axum/Cargo.toml`
- [x] Add `connectrpc-axum-core = { path = "../connectrpc-axum-core" }`

**Update**: `connectrpc-axum/src/lib.rs`
- [x] Re-export from core: `pub use connectrpc_axum_core::{Code, ConnectError, ...}`

**Update**: Internal imports throughout `connectrpc-axum/src/`
- [x] Replace local types with core imports

**Acceptance**: `cargo test -p connectrpc-axum` passes (all existing tests).

---

### Step 0.9: Add core to workspace

**Update**: `Cargo.toml` (workspace root)
- [x] Add `"connectrpc-axum-core"` to workspace members

**Acceptance**: `cargo build --workspace` succeeds.

---

## Phase 1: Client Crate - Unary Calls ✅

**Goal**: Basic client that can make unary RPC calls.

### Step 1.1: Create client crate scaffold ✅

```bash
mkdir -p connectrpc-axum-client/src
```

**Files to create**:
- [x] `connectrpc-axum-client/Cargo.toml`
- [x] `connectrpc-axum-client/src/lib.rs`

**Add to workspace**: Update root `Cargo.toml`

**Acceptance**: `cargo check -p connectrpc-axum-client` passes.

---

### Step 1.2: Implement response types ✅

**Create**: `connectrpc-axum-client/src/response.rs`

**Types**:
- [x] `ConnectResponse<T>` with `metadata` field
- [x] `into_inner()`, `metadata()`, `map()` methods
- [x] `Deref<Target=T>` implementation

**Acceptance**: Response wrapper compiles and is ergonomic.

---

### Step 1.3: Implement ClientBuilder ✅

**Create**: `connectrpc-axum-client/src/builder.rs`

**Types**:
- [x] `ClientBuilder` struct
- [x] `new(base_url)` constructor
- [x] `client(reqwest::Client)` method
- [x] `with_middleware(M)` method
- [x] `use_json()` / `use_proto()` methods
- [x] `compression(CompressionConfig)` method
- [x] `accept_encoding(CompressionEncoding)` method
- [x] `build() -> ConnectClient` method

**Acceptance**: Builder compiles with reqwest-middleware types.

---

### Step 1.4: Implement ConnectClient struct ✅

**Create**: `connectrpc-axum-client/src/client.rs`

**Types**:
- [x] `ConnectClient` struct with fields: `http`, `base_url`, `use_proto`, `compression`, `accept_encoding`
- [x] `builder(base_url) -> ClientBuilder` constructor

**Helper methods**:
- [x] `encode_message<T>(&self, msg: &T) -> Result<Bytes>` (proto or JSON)
- [x] `decode_message<T>(&self, bytes: &Bytes) -> Result<T>` (proto or JSON)
- [x] `unary_content_type(&self) -> &'static str`
- [x] `streaming_content_type(&self) -> &'static str`

**Acceptance**: Client struct compiles.

---

### Step 1.5: Implement call_unary ✅

**Update**: `connectrpc-axum-client/src/client.rs`

```rust
pub async fn call_unary<Req, Res>(
    &self,
    procedure: &str,
    request: &Req,
) -> Result<ConnectResponse<Res>, ConnectError>
```

**Implementation**:
- [x] Encode request body (proto or JSON)
- [x] Set headers: `content-type`, `connect-protocol-version: 1`
- [x] Optional: `content-encoding` if compression enabled
- [x] Optional: `accept-encoding` header
- [x] POST to `{base_url}/{procedure}`
- [x] Check response status
- [x] Parse error response if non-2xx
- [x] Decode successful response body
- [x] Extract metadata from response headers

**Acceptance**: Can make unary call to a test server.

---

### Step 1.6: Implement error response parsing ✅

**Create**: `connectrpc-axum-client/src/error_parser.rs`

**Functions**:
- [x] `parse_error_response(response: reqwest::Response) -> ConnectError`
- [x] Parse JSON error body: `{"code": "...", "message": "...", "details": [...]}`
- [x] Map HTTP status to Connect code as fallback

**Acceptance**: Error responses are properly parsed into `ConnectError::Status`.

---

### Step 1.7: Add unary integration test ✅

**Location**: `connectrpc-axum-examples/src/bin/client/unary-client.rs`

Integration tests live in the examples crate, not in `connectrpc-axum-client/tests/`. This follows the project convention where:
- Unit tests are in their respective modules (e.g., `builder::tests`, `client::tests`)
- Integration tests are executable binaries in `connectrpc-axum-examples/src/bin/client/`

**Tests** (7 total):
- [x] Unary call with JSON encoding
- [x] Unary call with Proto encoding
- [x] Default name handling
- [x] Response wrapper methods (Deref, map)
- [x] `into_parts()` method
- [x] Multiple sequential calls
- [x] Connection error handling (Transport error)

**Run with**:
```bash
# Start server, run tests, stop server
cargo run --bin connect-unary --no-default-features &
sleep 1
cargo run --bin unary-client --no-default-features
kill %1 2>/dev/null || true
```

**Acceptance**: All 7 tests pass ("=== All tests passed! ===" in output).

---

## Phase 2: Server Streaming ✅

**Goal**: Client can receive server-streaming responses.

### Step 2.1: Implement FrameDecoder ✅

**Create**: `connectrpc-axum-client/src/frame.rs`

**Struct**:
```rust
pub struct FrameDecoder<S, T> {
    stream: S,
    buffer: BytesMut,
    use_proto: bool,
    encoding: CompressionEncoding,
    trailers: Option<Metadata>,
    finished: bool,
    end_stream_error: Option<ConnectError>,
    _marker: PhantomData<T>,
}
```

**Implementation**:
- [x] `new(stream, use_proto, compression)` constructor
- [x] `Stream` trait implementation
- [x] Buffer management for partial frames
- [x] Bitmask flag parsing (0x01=compressed, 0x02=end-stream)
- [x] Decompression using codec from core
- [x] Message decoding (proto or JSON)
- [x] EndStream parsing (error → `Some(Err(...))`, success → store trailers, return `None`)
- [x] Unexpected EOF → `Some(Err(Code::DataLoss, ...))`
- [x] `trailers(&self) -> Option<&Metadata>` accessor

**Acceptance**: FrameDecoder correctly decodes envelope stream.

---

### Step 2.2: Implement StreamBody wrapper ✅

**Create**: `connectrpc-axum-client/src/stream_body.rs`

**Struct**:
```rust
pub struct StreamBody<S> {
    inner: S,
}
```

**Implementation**:
- [x] `new(inner)` constructor
- [x] `into_inner()` method
- [x] `trailers()` method for `StreamBody<FrameDecoder<S, T>>`
- [x] `Stream` trait delegation

**Acceptance**: StreamBody wraps decoder and provides trailers access.

---

### Step 2.3: Implement call_server_stream ✅

**Update**: `connectrpc-axum-client/src/client.rs`

```rust
pub async fn call_server_stream<Req, Res>(
    &self,
    procedure: &str,
    request: &Req,
) -> Result<ConnectResponse<StreamBody<FrameDecoder<impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin, Res>>>, ConnectError>
```

**Implementation**:
- [x] Encode request body (not envelope-wrapped for server streaming)
- [x] Set streaming content-type header
- [x] POST request
- [x] Check initial response status
- [x] Get compression encoding from `connect-content-encoding` header
- [x] Wrap `response.bytes_stream()` with FrameDecoder
- [x] Return wrapped in ConnectResponse + StreamBody

**Acceptance**: Can receive server-streaming response.

---

### Step 2.4: Add server streaming integration test ✅

**Location**: `connectrpc-axum-examples/src/bin/client/server-stream-client.rs`

Integration tests follow the examples convention - executable binaries that run against a test server.

**Tests** (7 total):
- [x] Server streams multiple messages with JSON encoding
- [x] Server streams multiple messages with Proto encoding
- [x] Server streams with default name
- [x] Connection error handling (Transport error)
- [x] Collect all messages with `collect()`
- [x] `is_finished()` works correctly
- [x] Trailers access after stream consumption

**Run with**:
```bash
# Start server, run tests, stop server
cargo run --bin connect-server-stream --no-default-features &
sleep 1
cargo run --bin server-stream-client --no-default-features
kill %1 2>/dev/null || true
```

**Acceptance**: All 7 tests pass ("=== All tests passed! ===" in output).

---

## Phase 3: Client Streaming ✅

**Goal**: Client can send streaming requests.

### Step 3.1: Implement FrameEncoder ✅

**Update**: `connectrpc-axum-client/src/frame.rs`

**Struct**:
```rust
pub struct FrameEncoder<S, T> {
    stream: S,
    use_proto: bool,
    encoding: CompressionEncoding,
    compression: CompressionConfig,
    state: EncoderState,
    _marker: PhantomData<T>,
}
```

**Implementation**:
- [x] `new(stream, use_proto, encoding, compression)` constructor
- [x] `Stream` trait implementation yielding `Result<Bytes, ConnectError>`
- [x] Message encoding (proto or JSON)
- [x] Optional compression (set 0x01 flag)
- [x] Envelope wrapping
- [x] Auto-send EndStream frame when inner stream exhausted
- [x] State machine for tracking encoder progress

**Acceptance**: FrameEncoder correctly encodes message stream.

---

### Step 3.2: Implement call_client_stream ✅

**Update**: `connectrpc-axum-client/src/client.rs`

```rust
pub async fn call_client_stream<Req, Res, S>(
    &self,
    procedure: &str,
    request: S,
) -> Result<ConnectResponse<Res>, ConnectError>
where
    Req: Message + Serialize + 'static,
    Res: Message + DeserializeOwned + Default,
    S: Stream<Item = Req> + Send + Unpin + 'static,
```

**Implementation**:
- [x] Wrap request stream with FrameEncoder
- [x] Create `reqwest::Body::wrap_stream(encoder)`
- [x] Set streaming content-type
- [x] POST request with streaming body
- [x] Response is also streaming format (single message + EndStream)
- [x] Use FrameDecoder to read single response message

**Acceptance**: Can send client-streaming request.

---

### Step 3.3: Add client streaming integration test ✅

**Location**: `connectrpc-axum-examples/src/bin/client/client-stream-client.rs`

Integration tests follow the examples convention - executable binaries that run against a test server.

**Tests** (7 total):
- [x] Client streams multiple messages with JSON encoding
- [x] Client streams multiple messages with Proto encoding
- [x] Client streams with single message
- [x] Client sends empty stream
- [x] Response wrapper methods (into_parts)
- [x] Connection error handling (Transport error)
- [x] Multiple sequential client streaming calls

**Run with**:
```bash
# Start server, run tests, stop server
cargo run --bin connect-client-stream --no-default-features &
sleep 1
cargo run --bin client-stream-client --no-default-features
kill %1 2>/dev/null || true
```

**Acceptance**: All 7 tests pass ("=== All tests passed! ===" in output).

---

## Phase 4: Bidi Streaming ✅

**Goal**: Full bidirectional streaming support.

### Step 4.1: Implement call_bidi_stream ✅

**Update**: `connectrpc-axum-client/src/client.rs`

```rust
pub async fn call_bidi_stream<Req, Res, S>(
    &self,
    procedure: &str,
    request: S,
) -> Result<ConnectResponse<StreamBody<FrameDecoder<impl Stream<...>, Res>>>, ConnectError>
where
    Req: Message + Serialize + 'static,
    Res: Message + DeserializeOwned + Default,
    S: Stream<Item = Req> + Send + Unpin + 'static,
```

**Implementation**:
- [x] Wrap request stream with FrameEncoder
- [x] Create streaming body
- [x] POST request
- [x] Wrap response stream with FrameDecoder
- [x] Return StreamBody wrapper

**Note**: Bidi requires HTTP/2. Documentation added to method docs.

**Acceptance**: Bidi streaming works over HTTP/2.

---

### Step 4.2: Add bidi streaming integration test ✅

**Location**: `connectrpc-axum-examples/src/bin/client/bidi-stream-client.rs`

Integration tests follow the examples convention - executable binaries that run against a test server.

**Tests** (7 total):
- [x] Bidi streams multiple messages with JSON encoding
- [x] Bidi streams multiple messages with Proto encoding
- [x] Bidi streams with single message
- [x] Connection error handling (Transport error)
- [x] Collect all messages with `collect()`
- [x] `is_finished()` works correctly
- [x] Trailers access after stream consumption

**Run with**:
```bash
# Start server, run tests, stop server
cargo run --bin connect-bidi-stream --no-default-features &
sleep 1
cargo run --bin bidi-stream-client --no-default-features
kill %1 2>/dev/null || true
```

**Acceptance**: All 7 tests pass ("=== All tests passed! ===" in output).

---

## Phase 5: Code Generation (Unary Only) ✅

**Goal**: `connectrpc-axum-build` generates typed client structs for unary RPCs.

**Note**: Streaming method generation deferred to Phase 5b after client streaming support (Phases 2-4) is complete.

### Step 5.1: Export ClientBuildError ✅

**Update**: `connectrpc-axum-client/src/lib.rs`

**Changes**:
- [x] Export `ClientBuildError` from builder module
- [x] Add `HttpClient` re-export (alias for `reqwest::Client`) for generated builders

**Acceptance**: Types available for generated client code.

---

### Step 5.2: Add type-state marker ✅

**Update**: `connectrpc-axum-build/src/lib.rs`

**Changes**:
- [x] Add `ConnectClient` type parameter to `CompileBuilder` (using existing `Enabled`/`Disabled` markers)
- [x] Default state is `Disabled`

**Acceptance**: Existing code compiles without changes (backwards compatible).

---

### Step 5.3: Add with_connect_client() method ✅

**Update**: `connectrpc-axum-build/src/lib.rs`

```rust
impl<C, T, TC> CompileBuilder<C, T, TC, Disabled> {
    pub fn with_connect_client(self) -> CompileBuilder<C, T, TC, Enabled> { ... }
}
```

**Acceptance**: Method available, no feature gate required.

---

### Step 5.4: Add client generation to AxumConnectServiceGenerator ✅

**Update**: `connectrpc-axum-build/src/gen.rs`

**Implementation**:
- [x] Add `include_connect_client` flag to `AxumConnectServiceGenerator`
- [x] Add `with_connect_client()` builder method
- [x] Add `generate_connect_client()` function that generates:
  - Service name constant (e.g., `HELLO_WORLD_SERVICE_SERVICE_NAME`)
  - Procedure path constants module (e.g., `hello_world_service_procedures`)
  - `{ServiceName}Client` struct with typed methods for unary RPCs
  - `{ServiceName}ClientBuilder` for configuration

**Method generation** (unary only for now):
- Unary: `async fn method(&self, request: &Req) -> Result<ConnectResponse<Res>, ConnectError>`
- Streaming methods: *Not generated yet - deferred to Phase 5b*

**Acceptance**: Generator produces valid Rust code for unary methods.

---

### Step 5.5: Generate client builder ✅

**Generated in**: `connectrpc-axum-build/src/gen.rs`

**Generated code**:
- [x] `{ServiceName}ClientBuilder` struct wrapping `ClientBuilder`
- [x] Configuration methods: `use_proto()`, `use_json()`, `client()`, `compression()`, etc.
- [x] `build() -> Result<{ServiceName}Client, ClientBuildError>` method

**Acceptance**: Builder pattern works for generated clients.

---

### Step 5.6: Integrate into compilation flow ✅

**Update**: `connectrpc-axum-build/src/lib.rs`

**In `compile()` method**:
- [x] If `ConnectClient = Enabled`, set `include_connect_client` flag on service generator
- [x] Client code appended to same output file as server code

**Acceptance**: Generated client code compiles with message types.

---

### Step 5.7: Add example ✅

**Create**: `connectrpc-axum-examples/src/bin/client/typed-client.rs`

**Example demonstrates**:
- [x] Creating typed client with `new()` and `builder()`
- [x] Calling typed methods (`say_hello()`, `get_greeting()`)
- [x] Using procedure path constants
- [x] Accessing underlying `ConnectClient`
- [x] Response wrapper methods

**Update**: `connectrpc-axum-examples/build.rs`
- [x] Add `.with_connect_client()` to enable client generation

**Acceptance**: Example compiles and demonstrates usage.

---

### Step 5.8: Verification ✅

**Tests**:
- [x] `cargo build -p connectrpc-axum-examples` succeeds
- [x] `cargo test -p connectrpc-axum-build` passes
- [x] `cargo test -p connectrpc-axum-client` passes

**Acceptance**: All tests pass.

---

## Phase 5b: Streaming Client Generation ✅

**Goal**: Generate typed methods for streaming RPCs.

**Prerequisites**: Phases 2-4 (client streaming support) must be complete.

### Step 5b.1: Server streaming methods ✅

- [x] Generate `call_server_stream` wrapper methods
- [x] Return type: `Result<ConnectResponse<StreamBody<FrameDecoder<...>>>, ConnectError>`

### Step 5b.2: Client streaming methods ✅

- [x] Generate `call_client_stream` wrapper methods
- [x] Take `impl Stream<Item = Req>` parameter

### Step 5b.3: Bidi streaming methods ✅

- [x] Generate `call_bidi_stream` wrapper methods
- [x] Take stream, return stream

---

## Final Checklist

**Completed**:
- [x] Phase 0: Core crate extraction
- [x] Phase 1: Client unary calls
- [x] Phase 2: Server streaming
- [x] Phase 3: Client streaming
- [x] Phase 4: Bidi streaming
- [x] Phase 5: Code generation (unary only)
- [x] `cargo test -p connectrpc-axum-build` passes
- [x] `cargo test -p connectrpc-axum-client` passes
- [x] `cargo build -p connectrpc-axum-examples` passes
- [x] Example: `connectrpc-axum-examples/src/bin/client/typed-client.rs`
- [x] Example: `connectrpc-axum-examples/src/bin/client/server-stream-client.rs`
- [x] Example: `connectrpc-axum-examples/src/bin/client/client-stream-client.rs`
- [x] Example: `connectrpc-axum-examples/src/bin/client/bidi-stream-client.rs`

**Remaining**:
- [x] Phase 5b: Streaming client code generation
- [x] `cargo test --workspace` passes (full workspace)
- [x] `cargo clippy --workspace` passes (with pre-existing warnings)
- [x] `cargo doc --workspace` builds
- [x] Update CHANGELOG.md
- [x] Update README.md with client usage examples

**All tasks complete!**

---

## Dependency Graph

```
Phase 0 (Core Extraction) ✅
    │
    ├──► Phase 1 (Unary Calls) ✅
    │        │
    │        ├──► Phase 5 (Unary Code Generation) ✅
    │        │
    │        └──► Phase 2 (Server Streaming) ✅
    │                 │
    │                 ├──► Phase 3 (Client Streaming) ✅
    │                 │        │
    │                 │        └──► Phase 4 (Bidi Streaming) ✅
    │                 │                 │
    │                 │                 └──► Phase 5b (Streaming Code Generation) ✅
    │                 │
    │                 └──► Phase 5b (Streaming Code Generation) ✅
    │
    └──► connectrpc-axum update (Step 0.8) ✅
```

**Current status**: All client streaming patterns and code generation are complete. Users can:
- Make unary RPC calls with both JSON and Proto encoding
- Receive server streaming responses with proper frame decoding
- Send client streaming requests with proper frame encoding
- Bidirectional streaming with both sending and receiving streams
- Access trailers from streaming responses
- Use generated typed client methods for all RPC types (unary, server streaming, client streaming, bidi)

**Next steps**: Update CHANGELOG.md and README.md with client usage examples.
