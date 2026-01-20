# Architecture

This page provides a technical overview of the connectrpc-axum library internals.

## Overview

connectrpc-axum bridges the Connect protocol with Axum's handler model. The core design principle is **separation of concerns**: protocol parsing happens in middleware, message encoding/decoding happens in extractors and response types, and your handlers stay focused on business logic.

```
HTTP Request → BridgeLayer → CompressionLayer → ConnectLayer → Handler → HTTP Response
                   ↓               ↓                 ↓
             Size limits,    HTTP body          Parse headers,
             streaming       compression        detect protocol
             detection       (unary only)
```

## Request Lifecycle

Understanding how a request flows through the library clarifies why each component exists.

### 1. Protocol Detection

The library first determines which protocol variant is in use:

| Method | Detection Source | Example |
|--------|------------------|---------|
| GET | `?encoding=` query param | `?encoding=proto` |
| POST | `Content-Type` header | `application/proto` |

This yields a `RequestProtocol` enum: `ConnectUnaryJson`, `ConnectUnaryProto`, `ConnectStreamJson`, or `ConnectStreamProto`.

For mixed Connect/gRPC deployments, `ContentTypeSwitch` routes by `Content-Type`:
- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum routes (Connect protocol)

gRPC compression is configured on the tonic service before it is added to `MakeServiceBuilder`. For gzip, enable it on the generated gRPC server:

```rust
use tonic::codec::CompressionEncoding;

let grpc_server = hello_world_service_server::HelloWorldServiceServer::new(tonic_service)
    .accept_compressed(CompressionEncoding::Gzip)
    .send_compressed(CompressionEncoding::Gzip);
```

You can also apply this via `MakeServiceBuilder::add_grpc_service_with`:

```rust
let app = MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_grpc_service_with(grpc_server, |svc| {
        svc.accept_compressed(CompressionEncoding::Gzip)
            .send_compressed(CompressionEncoding::Gzip)
    })
    .build();
```

### 2. Layer Stack Processing

The library uses a three-layer middleware stack when compression is enabled:

```
┌─────────────────────────────────────────────┐
│              BridgeLayer                    │  ← Size limit check, streaming detection
│  ┌───────────────────────────────────────┐  │
│  │     Tower CompressionLayer            │  │  ← HTTP body compression (unary only)
│  │  ┌─────────────────────────────────┐  │  │
│  │  │         ConnectLayer            │  │  │  ← Protocol negotiation, context
│  │  │  ┌───────────────────────────┐  │  │  │
│  │  │  │          Handler          │  │  │  │  ← Your RPC handlers
│  │  │  └───────────────────────────┘  │  │  │
│  │  └─────────────────────────────────┘  │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

**BridgeLayer** (outermost):
- Checks `Content-Length` against receive size limits (on compressed body)
- Detects Connect streaming requests (`application/connect+*`)
- For streaming: sets `Accept-Encoding: identity` to prevent Tower from compressing responses
- For streaming: removes `Content-Encoding` to prevent Tower from decompressing request body

**Tower CompressionLayer** (middle):
- Standard HTTP body compression for unary RPCs
- Uses `Accept-Encoding`/`Content-Encoding` headers
- Skipped for streaming (BridgeLayer sets identity encoding)

**ConnectLayer** (innermost):
- **Pre-protocol validation**: Checks content-type/encoding before protocol detection
  - Returns HTTP 415 with `Accept-Post` header for unsupported content-types
  - Uses `check_protocol_negotiation()` with `can_handle_content_type()` and `can_handle_get_encoding()`
- Parses `Content-Type` to determine encoding (JSON/Protobuf)
- Parses `?encoding=` query param for GET requests
- Validates `Connect-Protocol-Version` header when required
- Parses timeout from `Connect-Timeout-Ms` header
- Builds `ConnectContext` and stores it in request extensions

### 3. Handler Execution

Your handler receives a `ConnectRequest<T>` extractor that:
- Reads the HTTP body (respecting size limits)
- Decompresses if needed
- Decodes from JSON or Protobuf based on the detected protocol

Your handler returns a `ConnectResponse<T>` (or error) that:
- Encodes the response message per the request protocol
- Compresses if beneficial
- Sets appropriate headers

### 4. Pipeline Primitives

The `pipeline.rs` module provides the low-level functions used by extractors and response types:

**Request side:**
- `read_body` - Read HTTP body with size limit
- `decompress_bytes` - Decompress based on encoding
- `decode_proto` / `decode_json` - Decode message from bytes
- `process_envelope_payload` - Validate envelope flags and decompress payload
- `get_context_or_default` - Get `ConnectContext` from request extensions (with fallback)

**`RequestPipeline` methods:**
- `decode` - Decode from HTTP request (read body, decompress, decode)
- `decode_bytes` - Decode from raw bytes (decompress, check size, decode)
- `decode_enveloped_bytes` - Decode from enveloped bytes (for `application/connect+json` or `application/connect+proto`)

**`ResponsePipeline` methods:**
- `encode` - Encode response message to HTTP response (reads context from request extensions)
- `encode_with_context` - Encode with explicit context (when request not available)

**Response side:**
- `encode_proto` / `encode_json` - Encode message to bytes
- `compress_bytes` - Compress if beneficial (takes `&CompressionConfig` for level-aware compression)
- `wrap_envelope` - Wrap in Connect streaming frame
- `build_end_stream_frame` - Build EndStream frame

## Core Types

These are the types you interact with when building services:

### Layer Types

| Type | Purpose |
|------|---------|
| `BridgeLayer` | Bridges Tower compression with Connect streaming; enforces size limits |
| `BridgeService` | Service wrapper created by `BridgeLayer` |
| `ConnectLayer` | Protocol detection, context building, timeout handling |
| `ConnectService` | Service wrapper created by `ConnectLayer` |

### Context Types

| Type | Purpose |
|------|---------|
| `ConnectContext` | Protocol, compression, timeout, limits - set by layer, read by handlers |
| `RequestProtocol` | Enum identifying Connect variant (Unary/Stream x Json/Proto) |
| `MessageLimits` | Receive/send size limits (default: no limits) |
| `CompressionConfig` | Compression settings: `min_bytes` threshold and `level` (default: min_bytes=0, matching connect-go) |
| `CompressionContext` | Per-request compression context with envelope settings and full `CompressionConfig` |
| `CompressionEncoding` | Supported encodings: `Gzip`, `Deflate`, `Brotli`, `Zstd`, or `Identity`. Use `codec_with_level()` for level-aware compression |
| `CompressionLevel` | Compression level (re-exported from tower-http) |
| `EnvelopeCompression` | Per-envelope compression settings for streaming RPCs |
| `ContextError` | Error type for context building failures (protocol detection, header parsing) |
| `BoxedCodec` | Type-erased codec storage (`Box<dyn Codec>`) for dynamic compression dispatch |

### Request/Response Types

| Type | Purpose |
|------|---------|
| `ConnectRequest<T>` | Axum extractor - deserializes protobuf/JSON from request body |
| `ConnectRequest<Streaming<T>>` | Extractor for client/bidi streaming requests |
| `Streaming<T>` | Stream of messages from client (similar to Tonic's `Streaming<T>`) |
| `ConnectResponse<T>` | Response wrapper - encodes per detected protocol |
| `ConnectResponse<StreamBody<S>>` | Server streaming response wrapper |
| `StreamBody<S>` | Marker for streaming response bodies |

### Error Handling

| Type | Purpose |
|------|---------|
| `ConnectError` | Error with code, message, metadata, and optional details |
| `Code` | Connect/gRPC status codes (OK, InvalidArgument, NotFound, etc.) |
| `ErrorDetail` | Structured error detail with type URL and protobuf-encoded bytes |
| `ContextError` | Error bundled with protocol for proper response encoding |
| `ProtocolNegotiationError` | Pre-protocol error for HTTP 415 responses (unsupported content-type/encoding) |

Error details follow the Connect protocol's structured error format, serialized as JSON objects with `type` and `value` fields. The `ErrorDetail` type supports the `google.protobuf.Any` wire format.

`ProtocolNegotiationError` is used before protocol detection when the request cannot be handled. It produces raw HTTP 415 responses with an `Accept-Post` header listing supported content types (`SUPPORTED_CONTENT_TYPES`), bypassing Connect error formatting.

## Handler Wrappers

The library uses a unified handler wrapper that supports all RPC patterns:

| Wrapper | Use Case |
|---------|----------|
| `ConnectHandlerWrapper<F>` | Unified: unary, server/client/bidi streaming with optional extractors |
| `ConnectHandler<F>` | Type alias for `ConnectHandlerWrapper<F>` (convenience re-export) |
| `TonicCompatibleHandlerWrapper<F>` | Tonic-style unary with axum extractors |
| `TonicCompatibleStreamHandlerWrapper<F>` | Tonic-style server streaming with axum extractors |
| `TonicCompatibleClientStreamHandlerWrapper<F>` | Tonic-style client streaming with axum extractors |
| `TonicCompatibleBidiStreamHandlerWrapper<F>` | Tonic-style bidi streaming with axum extractors |

### Handler Functions

Two functions create method routers from handlers:

| Function | HTTP Method | Use Case |
|----------|-------------|----------|
| `post_connect(f)` | POST | Unary and streaming RPCs |
| `get_connect(f)` | GET | Idempotent unary RPCs (query param encoding) |

### How Handler Wrappers Work

`ConnectHandlerWrapper<F>` is a newtype that wraps a user function `F`. It has multiple `impl Handler<T, S>` blocks, each with different `where` bounds on `F`. The compiler selects the appropriate impl based on the handler signature.

| Pattern | Request Type | Response Type |
|---------|--------------|---------------|
| Unary | `ConnectRequest<Req>` | `ConnectResponse<Resp>` |
| Server streaming | `ConnectRequest<Req>` | `ConnectResponse<StreamBody<St>>` |
| Client streaming | `ConnectRequest<Streaming<Req>>` | `ConnectResponse<Resp>` |
| Bidi streaming | `ConnectRequest<Streaming<Req>>` | `ConnectResponse<StreamBody<St>>` |

The `T` parameter in `Handler<T, S>` acts as a discriminator tag for impl selection. Macro-generated implementations handle additional extractors for each pattern.

## Builder Pattern

The library uses a two-tier builder pattern to separate per-service concerns from infrastructure concerns.

### Generated Builders (per-service)

Generated at build time for each proto service. Handles handler registration, per-method routing, and state application.

### MakeServiceBuilder (library-level)

Combines multiple services and applies cross-cutting infrastructure. Supports two route types:
- `add_router()` - ConnectRPC routes that go through `ConnectLayer` for protocol handling
- `add_axum_router()` - Plain axum routes that bypass `ConnectLayer` (health checks, metrics, static files)

### Separation of Concerns

| Concern | Generated Builder | MakeServiceBuilder |
|---------|:-----------------:|:------------------:|
| Handler registration | ✓ | |
| Per-method routing | ✓ | |
| State application | ✓ | |
| Multi-service composition | | ✓ |
| ConnectLayer application | | ✓ |
| Plain HTTP routes | | ✓ |
| Message limits (receive/send) | | ✓ |
| Protocol header validation | | ✓ |
| Server-side timeout | | ✓ |
| gRPC service integration | | ✓ |
| FromRequestParts extraction | | ✓ |

## Wire Format

### Streaming Frame Format

5-byte envelope: `[flags: 1 byte][length: 4 bytes BE][payload]`

- Connect streaming uses EndStream flag (0x02) with JSON payload for trailing metadata
- gRPC uses HTTP trailers instead of EndStream frames

### Route Paths

Routes follow the pattern: `/<package>.<Service>/<Method>`

## Module Organization

```
connectrpc-axum/          # Core library
connectrpc-axum-build/    # Protobuf code generation
connectrpc-axum-examples/ # Examples and test clients
```

### Core Modules

| Module | Purpose |
|--------|---------|
| `handler.rs` | Handler wrappers implementing `axum::handler::Handler` |
| `layer/` | Middleware layers (see below) |
| `error.rs` | `ConnectError`, `ErrorDetail`, and `Code` types |
| `pipeline.rs` | Request/response primitives (decode, encode, compress) |
| `service_builder.rs` | Multi-service router composition |
| `tonic/` | Optional gRPC/Tonic interop module |

### layer/ module

| Module | Purpose |
|--------|---------|
| `bridge.rs` | `BridgeLayer`/`BridgeService` - bridges Tower compression with Connect streaming |
| `connect.rs` | `ConnectLayer`/`ConnectService` - protocol detection, context building, timeouts |

### tonic/ module (with `tonic` feature)

| Module | Purpose |
|--------|---------|
| `tonic.rs` | `ContentTypeSwitch` and `TonicCompatible` types  |
| `tonic/handler.rs` | Tonic-compatible handler wrappers, factory traits, and boxed call types |
| `tonic/parts.rs` | `RequestContext`, `CapturedParts`, and `FromRequestPartsLayer` |

The tonic module provides two key capabilities:

1. **Protocol switching**: `ContentTypeSwitch` routes by Content-Type header between gRPC and Connect
2. **Extractor support**: `FromRequestPartsLayer` captures HTTP request parts for use with axum's `FromRequestParts` extractors in tonic-style handlers

#### Boxed Call Types

The tonic module defines boxed callable types for generated service code:

| Type | Purpose |
|------|---------|
| `BoxedCall<Req, Resp>` | Unary RPC callable |
| `BoxedStreamCall<Req, Resp>` | Server streaming RPC callable |
| `BoxedClientStreamCall<Req, Resp>` | Client streaming RPC callable |
| `BoxedBidiStreamCall<Req, Resp>` | Bidirectional streaming RPC callable |
| `BoxedStream<T>` | Pinned boxed stream for streaming responses (`Pin<Box<dyn Stream<Item = Result<T, ConnectError>> + Send>>`) |

Each has corresponding factory traits (`IntoFactory`, `IntoStreamFactory`, `IntoClientStreamFactory`, `IntoBidiStreamFactory`) for adapting user handlers.

#### Handler Functions

| Function | Purpose |
|----------|---------|
| `post_tonic_unary(f)` | Creates POST router for tonic-compatible unary handlers |
| `post_tonic_stream(f)` | Creates POST router for tonic-compatible server streaming handlers |
| `post_tonic_client_stream(f)` | Creates POST router for tonic-compatible client streaming handlers |
| `post_tonic_bidi_stream(f)` | Creates POST router for tonic-compatible bidirectional streaming handlers |

#### Unimplemented Helpers

For generated code to provide default "unimplemented" methods:

| Function | Purpose |
|----------|---------|
| `unimplemented_boxed_call()` | Returns `BoxedCall` that returns `ConnectError::unimplemented` |
| `unimplemented_boxed_stream_call()` | Returns `BoxedStreamCall` that returns `ConnectError::unimplemented` |
| `unimplemented_boxed_client_stream_call()` | Returns `BoxedClientStreamCall` that returns `ConnectError::unimplemented` |
| `unimplemented_boxed_bidi_stream_call()` | Returns `BoxedBidiStreamCall` that returns `ConnectError::unimplemented` |

### context/ module

| Module | Purpose |
|--------|---------|
| `protocol.rs` | `RequestProtocol` enum, detection, and validation functions (`can_handle_content_type`, `can_handle_get_encoding`, `SUPPORTED_CONTENT_TYPES`) |
| `envelope_compression.rs` | `Codec` trait, per-envelope compression, `CompressionEncoding::codec_with_level()` for level-aware codecs |
| `limit.rs` | Receive and send message size limits |
| `timeout.rs` | Request timeout handling |
| `config.rs` | `ServerConfig` (crate-internal configuration) |
| `error.rs` | `ContextError` and `ProtocolNegotiationError` types |

#### Compression Architecture

The Connect protocol uses two different compression mechanisms depending on the RPC type:

**Unary RPCs** - HTTP Body Compression (Tower middleware):
- Uses standard `Accept-Encoding` / `Content-Encoding` headers
- Handled by Tower's `CompressionLayer` (gzip, br, deflate, zstd)
- `BridgeLayer` checks compressed body size before decompression
- No Connect-specific code needed

**Streaming RPCs** - Per-Envelope Compression (connectrpc-axum):
- Uses `Connect-Accept-Encoding` / `Connect-Content-Encoding` headers
- Each message envelope is individually compressed
- `BridgeLayer` prevents Tower from compressing/decompressing streaming bodies
- `envelope_compression.rs` provides `Codec` trait and built-in codecs

**Built-in Codecs** (for envelope compression):
- `GzipCodec` - always available
- `DeflateCodec` - requires `compression-deflate` feature
- `BrotliCodec` - requires `compression-br` feature
- `ZstdCodec` - requires `compression-zstd` feature

**Configuration** (matching connect-go):
- Default `min_bytes` is 0 (compress everything when compression is requested)
- `CompressionConfig::disabled()` sets threshold to `usize::MAX`

Response compression negotiation follows connect-go's approach: `negotiate_response_encoding()` uses first-match-wins (client preference order) and respects `q=0` (not acceptable) values.

### message/ module

| Module | Purpose |
|--------|---------|
| `request.rs` | `ConnectRequest<T>` and `Streaming<T>` extractors |
| `response.rs` | `ConnectResponse<T>` and streaming response encoding |

## Code Generation

The build crate uses a multi-pass approach to generate all necessary code.

### CompileBuilder Type-State Pattern

`CompileBuilder<Connect, Tonic, TonicClient>` uses phantom type parameters to enforce valid configurations at compile time:

| Parameter | `Enabled` | `Disabled` |
|-----------|-----------|------------|
| `Connect` | Generate Connect handlers | Types + serde only |
| `Tonic` | Generate tonic server stubs | No server stubs |
| `TonicClient` | Generate tonic client stubs | No client stubs |

The marker types (`Enabled`/`Disabled`) implement the `BuildMarker` trait, enabling compile-time configuration validation.

Default state: `CompileBuilder<Enabled, Disabled, Disabled>` (Connect handlers only).

Method availability is enforced via trait bounds:
- `no_handlers()` requires `Connect = Enabled`, transitions to `Connect = Disabled`
- `with_tonic()` requires `Connect = Enabled` and `Tonic = Disabled`
- `with_tonic_client()` requires `TonicClient = Disabled`

**Constraints:** `no_handlers()` and `with_tonic()` cannot be combined (enforced at compile time).

**Protoc Fetching:** The `fetch_protoc(version, path)` method downloads a protoc binary to the specified path using the protoc-fetcher crate. This is useful for CI environments or when a system protoc is unavailable.

### Pass 1: Prost + Connect

```
prost_build::Config
    ↓
├── Message/Enum types (Rust structs)
├── Connect service builders (if handlers enabled)
└── File descriptor set (for subsequent passes)
```

- User configuration via `with_prost_config()` is applied here
- All type customization (attributes, extern paths) must be done in this pass
- Generated builders use the unified `post_connect()` function which auto-detects RPC type from handler signature
- With `no_handlers()`, only message types are generated (no service builders)
- Streaming type aliases (`BoxedCall`, `BoxedStreamCall`, etc.) are only generated for RPC patterns actually used by the service

### Pass 1.5: Serde Implementations

```
pbjson-build
    ↓
└── Serde Serialize/Deserialize impls for all messages
```

- Uses the file descriptor set from Pass 1
- Handles `oneof` fields correctly with proper JSON representation

### Pass 2: Tonic Server Stubs (with `tonic` feature + `with_tonic()`)

```
tonic_prost_build::Builder
    ↓
└── Service traits ({Service} trait + {Service}Server)
```

- **Types are NOT regenerated** - uses `extern_path` to reference Pass 1 types
- Generated code is appended to the Pass 1 output files

### Pass 3: Tonic Client Stubs (with `tonic-client` feature + `with_tonic_client()`)

```
tonic_prost_build::Builder
    ↓
└── Client types ({service_name}_client::{Service}Client)
```

- **Types are NOT regenerated** - uses `extern_path` to reference Pass 1 types
- Can be used independently of `with_tonic()` (server stubs)
- Generated code is appended to the Pass 1 output files

