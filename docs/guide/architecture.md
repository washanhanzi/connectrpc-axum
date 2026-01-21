# Architecture

This page provides a conceptual overview of how connectrpc-axum works internally.

## Overview

connectrpc-axum bridges the Connect protocol with Axum's handler model through two crates:

| Crate | Purpose |
|-------|---------|
| `connectrpc-axum` | Runtime library - layers, extractors, response types |
| `connectrpc-axum-build` | Build-time code generation from proto files |

The core modules in the runtime library:

| Module | Purpose |
|--------|---------|
| `context/` | Protocol detection, compression config, message limits, timeouts |
| `pipeline.rs` | Request/response primitives (decode, encode, compress) |
| `layer/` | Middleware layers (`BridgeLayer`, `ConnectLayer`) |
| `message/` | `ConnectRequest<T>` extractor and `ConnectResponse<T>` wrapper |
| `handler.rs` | Handler wrappers that implement `axum::handler::Handler` |
| `tonic/` | Optional gRPC interop and extractor support |

## Request Lifecycle

### Layer Stack

Requests flow through a three-layer middleware stack:

```
HTTP Request
    ↓
┌─────────────────────────────────────────────┐
│              BridgeLayer                    │  ← Size limits, streaming detection
│  ┌───────────────────────────────────────┐  │
│  │     Tower CompressionLayer            │  │  ← HTTP body compression (unary only)
│  │  ┌─────────────────────────────────┐  │  │
│  │  │         ConnectLayer            │  │  │  ← Protocol detection, context
│  │  │  ┌───────────────────────────┐  │  │  │
│  │  │  │          Handler          │  │  │  │  ← Your RPC handlers
│  │  │  └───────────────────────────┘  │  │  │
│  │  └─────────────────────────────────┘  │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
    ↓
HTTP Response
```

**BridgeLayer** (outermost) - see `layer/bridge.rs`:
- Checks `Content-Length` against receive size limits (on compressed body)
- Detects Connect streaming requests (`application/connect+*`)
- For streaming: prevents Tower compression by setting identity encoding

**Tower CompressionLayer** (middle):
- Standard HTTP body compression for unary RPCs
- Uses `Accept-Encoding`/`Content-Encoding` headers

**ConnectLayer** (innermost) - see `layer/connect.rs`:
- Validates content-type and returns HTTP 415 for unsupported types
- Parses `Content-Type` to determine encoding (JSON/Protobuf)
- Parses `?encoding=` query param for GET requests
- Validates `Connect-Protocol-Version` header when required
- Parses timeout from `Connect-Timeout-Ms` header
- Builds `ConnectContext` and stores it in request extensions

### Compression Paths

The Connect protocol uses different compression mechanisms for unary vs streaming RPCs:

**Unary RPCs** - HTTP body compression:
```
Request → BridgeLayer (size check) → Tower decompress → ConnectLayer → Handler
                                                                          ↓
Response ← BridgeLayer ← Tower compress ← ConnectLayer ← Handler response
```
- Uses standard `Accept-Encoding` / `Content-Encoding` headers
- Handled entirely by Tower's `CompressionLayer`
- BridgeLayer checks compressed body size before decompression

**Streaming RPCs** - per-envelope compression:
```
Request → BridgeLayer (bypass Tower) → ConnectLayer → Handler
                                                          ↓
                                    Each message envelope compressed individually
```
- Uses `Connect-Accept-Encoding` / `Connect-Content-Encoding` headers
- BridgeLayer sets `Accept-Encoding: identity` to prevent Tower from interfering
- `context/envelope_compression.rs` provides codec implementations

### Code Structure

The request/response processing follows this module hierarchy:

```
context/           ← Configuration and protocol state
    protocol.rs        RequestProtocol enum, detection
    envelope_compression.rs    Per-message compression
    limit.rs           Message size limits
    timeout.rs         Request timeout
        ↓
pipeline.rs        ← Low-level encode/decode functions
        ↓
layer/             ← Middleware that builds context
    bridge.rs          BridgeLayer/BridgeService
    connect.rs         ConnectLayer/ConnectService
        ↓
message/           ← Axum extractors and response types
    request.rs         ConnectRequest<T>, Streaming<T>
    response.rs        ConnectResponse<T>, StreamBody<S>
```

Handlers receive a `ConnectRequest<T>` extractor that reads the `ConnectContext` from request extensions, then uses `pipeline.rs` functions to decode the message. Response encoding follows the reverse path.

### Axum Extractor Support in Tonic Handlers

When using tonic-compatible handlers, axum's `FromRequestParts` extractors need access to HTTP request parts. The challenge: tonic consumes the HTTP request before your handler runs.

The solution is `FromRequestPartsLayer` in `tonic/parts.rs`:

```
HTTP Request
    ↓
FromRequestPartsLayer ← Clones method, uri, version, headers into extensions
    ↓
Tonic gRPC Server    ← Consumes HTTP request, but extensions survive
    ↓
Your Handler         ← Reconstructs RequestContext from:
                        - CapturedParts (from extensions)
                        - extensions (from tonic::Request)
```

Key insight: `http::Extensions` cannot be cloned, but it can be *moved*. The layer captures clonable parts (`CapturedParts`), and the handler later combines them with the owned extensions to build a complete `RequestContext` for extraction.

## Code Generation

### ConnectHandlerWrapper

The `ConnectHandlerWrapper<F>` type transforms user functions into axum-compatible handlers. It's a newtype wrapper with multiple `impl Handler<T, S>` blocks, each with different trait bounds:

```
User function: async fn(E1, E2, ..., ConnectRequest<Req>) -> ConnectResponse<Resp>
               where E1, E2, ... : FromRequestParts
                                    ↓
            ConnectHandlerWrapper<F> implements Handler<T, S>
                                    ↓
                        Axum can route to it
```

Handlers can include any types implementing `FromRequestParts` before the `ConnectRequest<T>` parameter, just like regular axum handlers. The compiler selects the appropriate impl based on the handler signature:

| Pattern | Request Type | Response Type |
|---------|--------------|---------------|
| Unary | `ConnectRequest<Req>` | `ConnectResponse<Resp>` |
| Server streaming | `ConnectRequest<Req>` | `ConnectResponse<StreamBody<St>>` |
| Client streaming | `ConnectRequest<Streaming<Req>>` | `ConnectResponse<Resp>` |
| Bidi streaming | `ConnectRequest<Streaming<Req>>` | `ConnectResponse<StreamBody<St>>` |

See `handler.rs` for the implementation.

### Tonic-Compatible Handlers

For tonic-style handlers (trait-based), the library uses a factory pattern with boxed calls:

```
User trait impl: async fn method(&self, req: tonic::Request<Req>) -> Result<Response<Resp>, Status>
                                    ↓
            IntoFactory trait converts to BoxedCall
                                    ↓
            TonicCompatibleHandlerWrapper adapts to axum Handler
                                    ↓
                        Axum can route to it
```

The "2-layer box" approach (same pattern axum uses for `Handler` → `MethodRouter`):
1. **Factory layer**: `IntoFactory` trait produces `BoxedCall<Req, Resp>` - a type-erased callable
2. **Wrapper layer**: `TonicCompatibleHandlerWrapper` implements `Handler` for the boxed call

One caveat: axum uses a trait for the factory layer, while we use closures. See [this discussion](https://github.com/washanhanzi/connectrpc-axum/discussions/18) for the design rationale.

This allows generated code to work with user-provided trait implementations without knowing concrete types at compile time. See `tonic/handler.rs` for the boxed call types and factory traits.

### Two-Pass Prost Generation

Code generation uses two passes to avoid type duplication:

**Pass 1: Prost + Connect**
```
proto files → prost_build → Message types (Rust structs)
                          → Connect service builders
                          → File descriptor set
```

**Pass 2: Tonic (optional)**
```
File descriptor set → tonic_build → Service traits
                                  → Uses extern_path to reference Pass 1 types
```

The key is `extern_path`: Pass 2 doesn't regenerate types, it references the types from Pass 1. This keeps one source of truth for message types while allowing both Connect and gRPC code to coexist.

See `CompileBuilder` in `connectrpc-axum-build` for the type-state pattern that enforces valid configurations at compile time.

## MakeServiceBuilder

`MakeServiceBuilder` combines multiple services and applies cross-cutting infrastructure:

```rust
let app = MakeServiceBuilder::new()
    .add_router(hello_service_router)      // Connect service
    .add_router(echo_service_router)       // Another Connect service
    .add_grpc_service(grpc_server)         // Tonic gRPC service
    .add_axum_router(health_router)        // Plain axum routes (bypass ConnectLayer)
    .build();
```

The builder handles:
- Wrapping Connect routes with `ConnectLayer` for protocol handling
- Wrapping routes with `BridgeLayer` for compression bridging
- Adding Tower `CompressionLayer` for HTTP body compression
- Routing gRPC services through `ContentTypeSwitch` (by Content-Type header)
- Passing plain axum routes through without Connect processing

```
User provides:
    ├── Connect routers (from generated builders)
    ├── gRPC services (tonic)
    └── Plain axum routers

MakeServiceBuilder adds:
    ├── BridgeLayer
    ├── CompressionLayer
    ├── ConnectLayer (for Connect routes only)
    └── ContentTypeSwitch (routes gRPC vs Connect)

Output: Single axum Router
```

For mixed Connect/gRPC deployments, `ContentTypeSwitch` routes by `Content-Type` header:
- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum routes (Connect protocol)

See `service_builder.rs` for the implementation.
