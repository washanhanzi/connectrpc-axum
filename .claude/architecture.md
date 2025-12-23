# Architecture

## Overview

A Rust library implementing the [Connect RPC protocol](https://connectrpc.com/) for the Axum web framework. The library provides native handling of Connect protocol requests (unary and streaming) with automatic JSON/Protobuf encoding negotiation.

For gRPC and gRPC-web protocols, the library uses `ContentTypeSwitch` (in `tonic.rs`) to dispatch requests based on `Content-Type` header:
- `application/grpc*` requests → forwarded to a Tonic gRPC server
- All other requests → handled by Axum routes (Connect protocol)

This allows serving Connect, gRPC, and gRPC-web clients from a single HTTP endpoint.

## Workspace Structure

```
connectrpc-axum/          # Core library
connectrpc-axum-build/    # Protobuf code generation
connectrpc-axum-examples/ # Examples and test clients
connect-go/               # Official connect-go impl (reference)
```

## Core Modules

| Module | Purpose |
|--------|---------|
| `handler.rs` | Handler trait implementations for Axum integration |
| `request.rs` | `ConnectRequest<T>` extractor - body parsing |
| `response.rs` | `ConnectResponse<T>` encoding per protocol |
| `protocol.rs` | Protocol detection + task-local context |
| `layer.rs` | `ConnectLayer` middleware |
| `error.rs` | `ConnectError` and `Code` types |
| `service_builder.rs` | Multi-service router composition |
| `stream_response.rs` | Server streaming response handling |
| `tonic.rs` | Optional gRPC/Tonic interop |

## Key Types

```rust
ConnectRequest<T>    // Axum extractor - deserializes protobuf/JSON
ConnectResponse<T>   // Response wrapper - encodes per protocol
StreamBody<S>        // Marks streaming responses
ConnectError         // Error with code, message, details
RequestProtocol      // Enum: ConnectUnary{Json,Proto}, ConnectStream{Json,Proto}, GrpcProto
```

## Protocol Detection

`ConnectLayer` middleware detects protocol from:
- GET: `?encoding=proto|json` query param
- POST: `Content-Type` header

Protocol stored in task-local context, accessed by request/response handlers.

## Frame Format

Streaming uses 5-byte envelope:
```
[flags: 1 byte][length: 4 bytes BE][payload]
```

- Connect streaming: EndStream flag (0x02) with JSON payload
- gRPC: Uses HTTP trailers instead

## Handler Pattern

Handlers wrap user functions to implement Axum's `Handler` trait:

```rust
async fn my_handler(
    State(ctx): State<AppState>,   // Axum extractors first
    Query(q): Query<Params>,
    ConnectRequest(req): ConnectRequest<MyProto>,  // Body last
) -> Result<ConnectResponse<MyResp>, ConnectError>
```

Macro-generated impls support up to 16 extractors before `ConnectRequest<T>`.

## Code Generation Flow

1. `build.rs` calls `connectrpc_axum_build::compile_dir("proto")`
2. Generates service builders with fluent API
3. Runtime: `ServiceBuilder::new().handler(fn).with_state(s).build()`

## Design Decisions

- **Task-local protocol context**: Avoids threading parameters through extractors
- **Newtype wrappers**: Type-safe request/response/streaming distinction
- **Axum integration**: Leverages existing extractor ecosystem
- **Optional Tonic feature**: Single-port gRPC + Connect serving
- **Build-time codegen**: Type-safe service builders with IDE support
