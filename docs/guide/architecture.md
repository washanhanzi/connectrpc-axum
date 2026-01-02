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

Before your handler runs, `ConnectLayer` parses headers and builds a `Context`:

- Parses `Content-Type` to determine encoding (JSON/Protobuf)
- Parses `?encoding=` query param for GET requests
- Validates `Connect-Protocol-Version` header when required
- Extracts compression encoding from headers
- Stores the `Context` in request extensions

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
| `Context` | Protocol, compression, timeout, limits - set by layer, read by handlers |
| `RequestProtocol` | Enum identifying Connect variant (Unary/Stream × Json/Proto) |
| `MessageLimits` | Max message size configuration (default 4MB) |

### Request/Response Types

| Type | Purpose |
|------|---------|
| `ConnectRequest<T>` | Axum extractor - deserializes protobuf/JSON from request body |
| `ConnectStreamingRequest<T>` | Extractor for client streaming requests |
| `ConnectResponse<T>` | Response wrapper - encodes per detected protocol |
| `ConnectStreamResponse<S>` | Server streaming response wrapper |
| `StreamBody<S>` | Marker for streaming response bodies |

### Error Handling

| Type | Purpose |
|------|---------|
| `ConnectError` | Error with code, message, and optional details |
| `Code` | Connect/gRPC status codes (OK, InvalidArgument, NotFound, etc.) |

## Handler Wrappers

These implement `axum::handler::Handler` for each RPC pattern:

| Wrapper | Use Case |
|---------|----------|
| `ConnectHandlerWrapper<F>` | Unary requests |
| `ConnectStreamHandlerWrapper<F>` | Server streaming |
| `ConnectClientStreamHandlerWrapper<F>` | Client streaming |
| `ConnectBidiStreamHandlerWrapper<F>` | Bidirectional streaming |

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
| `error.rs` | `ConnectError` and `Code` types |
| `pipeline.rs` | Request/response primitives (decode, encode, compress) |
| `service_builder.rs` | Multi-service router composition |
| `tonic.rs` | Optional gRPC/Tonic interop |

### context/ module

| Module | Purpose |
|--------|---------|
| `protocol.rs` | `RequestProtocol` enum and detection |
| `compression.rs` | Compression encoding and functions |
| `limit.rs` | Message size limits |
| `timeout.rs` | Request timeout handling |

### message/ module

| Module | Purpose |
|--------|---------|
| `request.rs` | `ConnectRequest<T>` extractor |
| `response.rs` | `ConnectResponse<T>` encoding |
| `stream.rs` | Streaming types and frame handling |

## Code Generation

**Base generation:**
1. `prost_build::Config` generates protobuf types + service builders
2. `pbjson-build` generates serde implementations

**With Tonic feature:**
1. Additional pass generates tonic server stubs
2. Uses `extern_path` to reference existing prost types

