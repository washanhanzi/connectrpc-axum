# Architecture

This page provides a technical overview of the connectrpc-axum library internals.

## Overview

connectrpc-axum bridges the Connect protocol with Axum's handler model. The core design principle is **separation of concerns**: protocol parsing happens in middleware, message encoding/decoding happens in extractors and response types, and your handlers stay focused on business logic.

```
HTTP Request → ConnectLayer → Handler(ConnectRequest<T>) → ConnectResponse<T> → HTTP Response
                    ↓                     ↓
              Parse headers,        Decode body,
              detect protocol       encode response
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

### 2. Middleware Processing (ConnectLayer)

Before your handler runs, `ConnectLayer` parses headers and builds a `ConnectContext`:

- Parses `Content-Type` to determine encoding (JSON/Protobuf)
- Parses `?encoding=` query param for GET requests
- Validates `Connect-Protocol-Version` header when required
- Extracts compression encoding from headers
- Stores the `ConnectContext` in request extensions

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
- `unwrap_envelope` - Unwrap Connect streaming frame
- `get_context_or_default` - Get `ConnectContext` from request extensions (with fallback)

**`RequestPipeline` methods:**
- `decode` - Decode from HTTP request (read body, decompress, decode)
- `decode_bytes` - Decode from raw bytes (decompress, check size, decode)
- `decode_enveloped_bytes` - Decode from enveloped bytes (for `application/connect+json` or `application/connect+proto`)

**Response side:**
- `encode_proto` / `encode_json` - Encode message to bytes
- `compress_bytes` - Compress if beneficial
- `wrap_envelope` - Wrap in Connect streaming frame
- `build_end_stream_frame` - Build EndStream frame

## Core Types

These are the types you interact with when building services:

### Context Types

| Type | Purpose |
|------|---------|
| `ConnectContext` | Protocol, compression, timeout, limits - set by layer, read by handlers |
| `RequestProtocol` | Enum identifying Connect variant (Unary/Stream × Json/Proto) |
| `MessageLimits` | Max message size configuration (default 4MB) |
| `Codec` | Trait for compression/decompression (implement for custom algorithms) |
| `GzipCodec` | Built-in gzip compression codec |
| `IdentityCodec` | Built-in no-op codec (zero-copy passthrough) |

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

Error details follow the Connect protocol's structured error format, serialized as JSON objects with `type` and `value` fields. The `ErrorDetail` type supports the `google.protobuf.Any` wire format.

## Handler Wrappers

The library uses a unified handler wrapper that supports all RPC patterns:

| Wrapper | Use Case |
|---------|----------|
| `ConnectHandlerWrapper<F>` | Unified: unary, server/client/bidi streaming with optional extractors |
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

`ConnectHandlerWrapper<F>` is a newtype that wraps a user function `F`. It has multiple `impl Handler<T, S>` blocks, each with different `where` bounds on `F`. The compiler selects the appropriate impl based on the handler signature:

**Unary handlers:**
```rust
// Basic: (ConnectRequest<Req>) -> ConnectResponse<Resp>
// With extractors: (State<T>, ConnectRequest<Req>) -> ConnectResponse<Resp>
```

**Server streaming handlers:**
```rust
// Basic: (ConnectRequest<Req>) -> ConnectResponse<StreamBody<St>>
// With extractors: (State<T>, ConnectRequest<Req>) -> ConnectResponse<StreamBody<St>>
```

**Client streaming handlers:**
```rust
// Basic: (ConnectRequest<Streaming<Req>>) -> ConnectResponse<Resp>
// With extractors: (State<T>, ConnectRequest<Streaming<Req>>) -> ConnectResponse<Resp>
```

**Bidi streaming handlers:**
```rust
// Basic: (ConnectRequest<Streaming<Req>>) -> ConnectResponse<StreamBody<St>>
// With extractors: (State<T>, ConnectRequest<Streaming<Req>>) -> ConnectResponse<StreamBody<St>>
```

The `T` parameter in `Handler<T, S>` acts as a discriminator tag for impl selection. Separate macro-generated implementations handle extractors for each streaming pattern.

## Builder Pattern

The library uses a two-tier builder pattern to separate per-service concerns from infrastructure concerns.

### Generated Builders (per-service)

Generated at build time for each proto service. Handles handler registration and routing:

```rust
HelloWorldServiceBuilder::new()
    .say_hello(handler)
    .with_state(app_state)
    .build()          // Returns bare Router
    .build_connect()  // Returns Router with ConnectLayer
```

### MakeServiceBuilder (library-level)

Combines multiple services and applies cross-cutting infrastructure:

```rust
MakeServiceBuilder::new()
    .add_router(hello_router)           // ConnectRPC routes (with ConnectLayer)
    .add_router(user_router)
    .add_axum_router(health_router)     // Plain HTTP routes (bypass ConnectLayer)
    .message_limits(MessageLimits::new(16 * 1024 * 1024))
    .require_protocol_header(true)
    .timeout(Duration::from_secs(30))   // server-side timeout
    .add_grpc_service(grpc_svc)         // optional gRPC service
    .build()
```

**Route types:**
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
| Message limits | | ✓ |
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
| `layer.rs` | `ConnectLayer` middleware |
| `error.rs` | `ConnectError`, `ErrorDetail`, and `Code` types |
| `pipeline.rs` | Request/response primitives (decode, encode, compress) |
| `service_builder.rs` | Multi-service router composition |
| `tonic/` | Optional gRPC/Tonic interop module |

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

Each has corresponding factory traits (`IntoFactory`, `IntoStreamFactory`, `IntoClientStreamFactory`, `IntoBidiStreamFactory`) for adapting user handlers.

### context/ module

| Module | Purpose |
|--------|---------|
| `protocol.rs` | `RequestProtocol` enum and detection |
| `compression.rs` | `Codec` trait, `GzipCodec`, `IdentityCodec`, compression functions |
| `limit.rs` | Message size limits |
| `timeout.rs` | Request timeout handling |

#### Compression Architecture

The `compression.rs` module provides a `Codec` trait for compression/decompression:

```rust
pub trait Codec: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn compress(&self, data: Bytes) -> io::Result<Bytes>;
    fn decompress(&self, data: Bytes) -> io::Result<Bytes>;
}
```

Built-in implementations:
- `IdentityCodec`: Zero-copy passthrough (no compression)
- `GzipCodec`: Gzip compression via flate2

The `default_codec()` function returns the appropriate codec for a `CompressionEncoding`. Custom codecs (zstd, brotli, etc.) can implement the `Codec` trait.

Response compression negotiation follows RFC 7231: `negotiate_response_encoding()` parses `Accept-Encoding` headers respecting client preference order and `q=0` (not acceptable) values.

### message/ module

| Module | Purpose |
|--------|---------|
| `request.rs` | `ConnectRequest<T>` and `Streaming<T>` extractors |
| `response.rs` | `ConnectResponse<T>` and streaming response encoding |

## Code Generation

The build crate uses a multi-pass approach to generate all necessary code.

### Pass 1: Prost + Connect (always)

```
prost_build::Config
    ↓
├── Message/Enum types (Rust structs)
├── Connect service builders ({Service}ServiceBuilder)
└── File descriptor set (for Pass 1.5)
```

- User configuration via `with_prost_config()` is applied here
- All type customization (attributes, extern paths) must be done in this pass
- Generated builders use the unified `post_connect()` function which auto-detects RPC type from handler signature

### Pass 1.5: Serde Implementations (always)

```
pbjson-build
    ↓
└── Serde Serialize/Deserialize impls for all messages
```

- Uses the file descriptor set from Pass 1
- Handles `oneof` fields correctly with proper JSON representation

### Pass 2: Tonic Server Stubs (with `tonic` feature)

```
tonic_prost_build::Builder
    ↓
└── Service traits ({Service} trait + {Service}Server)
```

- **Types are NOT regenerated** - uses `extern_path` to reference Pass 1 types
- User configuration via `with_tonic_prost_config()` only affects service generation
- Internal overrides (cannot be changed by user):
  - `build_client(false)` - no client code
  - `build_server(true)` - generate server traits
  - `compile_well_known_types(false)` - use extern paths

### Configuration Separation

| Method | Pass | Affects |
|--------|------|---------|
| `with_prost_config()` | 1 | Message types, enum types, extern paths |
| `with_tonic_prost_config()` | 2 | Service trait generation only |

**Example:**

```rust
connectrpc_axum_build::compile_dir("proto")
    .with_prost_config(|config| {
        // Configure types here (Pass 1)
        config.type_attribute("MyMessage", "#[derive(Hash)]");
        config.extern_path(".google.protobuf", "::pbjson_types");
    })
    .with_tonic()
    .compile()?;
```

