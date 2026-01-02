# Architecture

This page provides a technical overview of the connectrpc-axum library internals.

## Library Structure

```
connectrpc-axum/          # Core library
connectrpc-axum-build/    # Protobuf code generation
connectrpc-axum-examples/ # Examples and test clients
```

## Module Overview

### Core Modules

| Module | Purpose |
|--------|---------|
| `handler.rs` | Handler wrappers implementing `axum::handler::Handler` |
| `layer.rs` | `ConnectLayer` middleware |
| `error.rs` | `ConnectError` and `Code` types |
| `pipeline.rs` | Request/response primitives (decode, encode, compress) |
| `service_builder.rs` | Multi-service router composition |
| `tonic.rs` | Optional gRPC/Tonic interop |

### context/ module

Handles request context extracted by middleware:

| Module | Purpose |
|--------|---------|
| `protocol.rs` | `RequestProtocol` enum and detection |
| `compression.rs` | Compression encoding and functions |
| `limit.rs` | Message size limits |
| `timeout.rs` | Request timeout handling |

### message/ module

Request and response types:

| Module | Purpose |
|--------|---------|
| `request.rs` | `ConnectRequest<T>` extractor |
| `response.rs` | `ConnectResponse<T>` encoding |
| `stream.rs` | Streaming types and frame handling |

## Layered Design

The library separates concerns into distinct layers:

### 1. ConnectLayer (Middleware)

Runs before handlers to parse headers and build context:

- Parses `Content-Type` to determine encoding (JSON/Protobuf)
- Parses `?encoding=` query param for GET requests
- Validates `Connect-Protocol-Version` header when required
- Stores `Context` in request extensions

### 2. Pipeline (Processing)

The `pipeline.rs` module provides primitive functions:

**Request primitives:**
- `read_body` - Read HTTP body with size limit
- `decompress_bytes` - Decompress based on encoding
- `decode_proto` / `decode_json` - Decode message from bytes
- `unwrap_envelope` - Unwrap Connect streaming frame

**Response primitives:**
- `encode_proto` / `encode_json` - Encode message to bytes
- `compress_bytes` - Compress if beneficial
- `wrap_envelope` - Wrap in Connect streaming frame
- `build_end_stream_frame` - Build EndStream frame

### 3. Request/Response Flow

```
Request Flow:
  HTTP Request → ConnectLayer (parse headers, build context)
               → ConnectRequest<T> extractor (decode body)
               → Handler function

Response Flow:
  Handler Result → ConnectResponse<T> (encode per protocol)
                 → HTTP Response
```

## Key Types

```rust
// Context (set by layer, used by handlers)
Context                    // Protocol, compression, timeout, limits

// Request/Response
ConnectRequest<T>          // Axum extractor - deserializes protobuf/JSON
ConnectStreamingRequest<T> // Client streaming request extractor
ConnectResponse<T>         // Response wrapper - encodes per protocol
ConnectStreamResponse<S>   // Server streaming response wrapper
StreamBody<S>              // Marks streaming responses

// Error handling
ConnectError               // Error with code, message, details

// Protocol detection
RequestProtocol            // ConnectUnary{Json,Proto}, ConnectStream{Json,Proto}

// Configuration
MessageLimits              // Max message size (default 4MB)
```

## Handler Wrappers

| Wrapper | Use Case |
|---------|----------|
| `ConnectHandlerWrapper<F>` | Unary requests |
| `ConnectStreamHandlerWrapper<F>` | Server streaming |
| `ConnectClientStreamHandlerWrapper<F>` | Client streaming |
| `ConnectBidiStreamHandlerWrapper<F>` | Bidirectional streaming |

## Two-Tier Builder Pattern

### Generated Builders (per-service)

Generated at build time for each proto service:

```rust
HelloWorldServiceBuilder::new()
    .say_hello(handler)
    .with_state(app_state)
    .build()          // Returns bare Router
    .build_connect()  // Returns Router with ConnectLayer
```

### MakeServiceBuilder (library-level)

Combines multiple services and applies infrastructure:

```rust
MakeServiceBuilder::new()
    .add_router(hello_router)
    .add_router(user_router)
    .message_limits(MessageLimits::new(16 * 1024 * 1024))
    .require_protocol_header(true)
    .add_grpc_service(grpc_svc)  // optional
    .build()
```

### Separation of Concerns

| Concern | Generated Builder | MakeServiceBuilder |
|---------|:-----------------:|:------------------:|
| Handler registration | ✓ | |
| Per-method routing | ✓ | |
| State application | ✓ | |
| Multi-service composition | | ✓ |
| ConnectLayer application | | ✓ |
| Message limits | | ✓ |
| Protocol header validation | | ✓ |
| gRPC service integration | | ✓ |

## Protocol Detection

Connect protocol is detected from:
- **GET**: `?encoding=proto|json` query param
- **POST**: `Content-Type` header

For gRPC/gRPC-web, `ContentTypeSwitch` dispatches by `Content-Type`:
- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum routes (Connect protocol)

## Frame Format (Streaming)

5-byte envelope: `[flags: 1 byte][length: 4 bytes BE][payload]`

- Connect streaming uses EndStream flag (0x02) with JSON payload
- gRPC uses HTTP trailers instead

## Code Generation

**Base generation:**
1. `prost_build::Config` generates protobuf types + service builders
2. `pbjson-build` generates serde implementations

**With Tonic feature:**
1. Additional pass generates tonic server stubs
2. Uses `extern_path` to reference existing prost types

Route paths follow the pattern: `/<package>.<Service>/<Method>`
