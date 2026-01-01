# Architecture

## Overview

A Rust library implementing the [Connect RPC protocol](https://connectrpc.com/) for Axum. Provides native handling of Connect protocol requests (unary and streaming) with automatic JSON/Protobuf encoding negotiation.

For gRPC/gRPC-web, `ContentTypeSwitch` dispatches by `Content-Type`:
- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum routes (Connect protocol)

## Workspace Structure

```
connectrpc-axum/          # Core library
connectrpc-axum-build/    # Protobuf code generation
connectrpc-axum-examples/ # Examples and test clients
```

## Core Modules

| Module | Purpose |
|--------|---------|
| `handler.rs` | `ConnectHandlerWrapper<F>` implements `axum::handler::Handler` |
| `request.rs` | `ConnectRequest<T>` extractor - body parsing |
| `response.rs` | `ConnectResponse<T>` encoding per protocol |
| `protocol.rs` | Protocol detection from request headers/query |
| `layer.rs` | `ConnectLayer` middleware |
| `error.rs` | `ConnectError` and `Code` types |
| `limits.rs` | `MessageLimits` for max message size configuration |
| `service_builder.rs` | Multi-service router composition |
| `stream_response.rs` | Server streaming response handling |
| `tonic.rs` | Optional gRPC/Tonic interop, `ContentTypeSwitch` |

## Layered Architecture

The library separates concerns into distinct layers:

### Context Model (`Context`)

Contains core logic for request handling. Extracted by the layer and consumed by handlers.

- **Protocol**: `RequestProtocol` enum
- **Compression**: `CompressionContext` for encoding/decoding
- **Timeout**: Optional request timeout
- **Message limits**: Max message size for decode operations
- **require_protocol_header**: Whether to enforce Connect-Protocol-Version header

### ConnectLayer (Middleware)

Responsible for header parsing and validation. Runs before handlers.

- Parses `Content-Type` header to determine encoding (JSON/Protobuf)
- Parses `?encoding=` query param for GET requests
- Validates `Connect-Protocol-Version` header when required
- Stores `ConnectContext` in request extensions for downstream use

### Request/Response Pipeline

Handlers compose decode/encode functions in a pipeline:

```
Request Flow:
  HTTP Request → ConnectLayer (parse headers, build context)
               → ConnectRequest<T> extractor (decode body using context)
               → Handler function

Response Flow:
  Handler Result → ConnectResponse<T> (encode using protocol from context)
                 → HTTP Response
```

**Pipeline composition in extractors/responses:**
- `ConnectRequest<T>`: Reads `ConnectContext` from extensions, decodes body (JSON or Protobuf)
- `ConnectResponse<T>`: Reads protocol from extensions, encodes response accordingly
- Streaming variants follow same pattern with frame envelope handling

## Key Types

```rust
// Context (set by layer, used by handlers)
Context                    // Protocol, compression, timeout, limits - stored in request extensions

// Request/Response
ConnectRequest<T>          // Axum extractor - deserializes protobuf/JSON
ConnectStreamingRequest<T> // Client streaming request extractor
ConnectResponse<T>         // Response wrapper - encodes per protocol
ConnectStreamResponse<S>   // Server streaming response wrapper
StreamBody<S>              // Marks streaming responses

// Error handling
ConnectError               // Error with code, message, details

// Protocol detection (gRPC handled separately by ContentTypeSwitch)
RequestProtocol            // Enum: ConnectUnary{Json,Proto}, ConnectStream{Json,Proto}, Unknown

// Message limits
MessageLimits              // Max message size config (default 4MB)

// Handler wrappers
ConnectHandlerWrapper<F>              // Unary handler
ConnectStreamHandlerWrapper<F>        // Server streaming handler
ConnectClientStreamHandlerWrapper<F>  // Client streaming handler
ConnectBidiStreamHandlerWrapper<F>    // Bidirectional streaming handler
```

## Handler Pattern

**Extractor rule:** Any `FromRequestParts<S>` extractors first, `ConnectRequest<Req>` must be last.

**Handler signature:**
```rust
F: Fn(...parts..., ConnectRequest<Req>) -> impl Future<Output = Result<ConnectResponse<Resp>, ConnectError>>
```
- `Req`: `prost::Message + serde::de::DeserializeOwned + Default + Send + Sync + 'static`
- `Resp`: `prost::Message + serde::Serialize + Send + Clone + Sync + 'static`

**Example:**
```rust
use axum::{extract::{Query, State}, Router};
use connectrpc_axum::prelude::*;

async fn say_hello(
    Query(_p): Query<Pagination>,
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hello, {}!", req.name.unwrap_or_default()) }))
}

let router = Router::new().route(
    "/hello.HelloWorldService/SayHello",
    connectrpc_axum::post_connect(say_hello),
);
```

**Request/Response behavior:**
- `ConnectRequest<T>`: POST with `application/proto` or `application/json`; GET via query params (`connect=v1`, `encoding`, `message`, optional `base64`)
- `ConnectResponse<T>`: Implements `IntoResponse` (JSON by default)

## Tonic-Compatible Mode

For generated gRPC servers, `TonicCompatibleHandlerWrapper<F>` restricts handlers to:
- `(ConnectRequest<Req>)` with state `()`
- `(State<S>, ConnectRequest<Req>)` with generic state `S`

**Generated types:**
- `*ServiceTonicCompatibleBuilder` - adds routes and handler factories
- `*TonicService` - implements `<service>_server::<Service>` trait

**Example:**
```rust
let (router, svc) = HelloWorldServiceTonicCompatibleBuilder::new()
    .say_hello(say_hello)
    .with_state(app_state)
    .build();

let grpc = hello_world_service_server::HelloWorldServiceServer::new(svc);
let dispatch = connectrpc_axum::MakeServiceBuilder::new()
    .add_router(router)
    .add_grpc_service(grpc)
    .build();
axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
```

## Protocol Detection

`ConnectLayer` detects protocol from:
- GET: `?encoding=proto|json` query param
- POST: `Content-Type` header

## Frame Format (Streaming)

5-byte envelope: `[flags: 1 byte][length: 4 bytes BE][payload]`
- Connect streaming: EndStream flag (0x02) with JSON payload
- gRPC: Uses HTTP trailers instead

## Code Generation

**Base (always):**
1. Pass 1: `prost_build::Config` generates protobuf types + `AxumConnectServiceGenerator` service builders, emits `descriptor.bin`
2. Pass 1.5: `pbjson-build` generates serde `impl` blocks (handles oneof correctly), appends to generated files

**Tonic feature (additional pass):**
1. Pass 2: tonic server stubs only, using `extern_path` to reference existing prost types (avoids duplication)
2. Append server code to `<proto>.rs`, clean up temp files

Route paths: `/<package>.<Service>/<Method>`

## Two-Tier Builder Architecture

The library uses two levels of builders with clear separation of concerns:

1. **Generated Builders** → Build routes (handler registration, per-service routing)
2. **MakeServiceBuilder** → Apply ConnectLayer to routes (cross-cutting infrastructure)

### Generated Builders (per-service)

Generated at build time for each proto service. Responsible for registering handlers.

```rust
// Generated: {Service}ServiceBuilder
pub struct HelloWorldServiceBuilder<S = ()> {
    pub router: axum::Router<S>,
}

impl HelloWorldServiceBuilder<S> {
    pub fn new() -> Self { ... }
    pub fn say_hello<F, T>(self, handler: F) -> Self { ... }  // per-method
    pub fn with_state<S2>(self, state: S) -> Self { ... }
    pub fn build(self) -> axum::Router<()> {
        self.router  // Returns bare router, no layer applied
    }
    pub fn build_connect(self) -> axum::Router<()> {
        self.router.layer(ConnectLayer::new())  // Convenience: applies ConnectLayer
    }
}
```

**Output:**
- `build()`: Bare `axum::Router` (for use with `MakeServiceBuilder`)
- `build_connect()`: Router with `ConnectLayer` applied (standalone convenience)

### MakeServiceBuilder (library-level)

Defined in `service_builder.rs`. Combines multiple service routers and applies infrastructure.

```rust
MakeServiceBuilder::new()
    .add_router(hello_router)      // from HelloWorldServiceBuilder.build()
    .add_router(user_router)       // from UserServiceBuilder.build()
    .message_limits(MessageLimits::new(16 * 1024 * 1024))
    .require_protocol_header(true)
    .add_grpc_service(grpc_svc)    // optional tonic services
    .build()                       // applies ConnectLayer here
```

**Output:** Final service with `ConnectLayer` applied (protocol detection, limits, header validation).

### Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│  Generated Builders (one per proto service)                 │
│                                                             │
│  HelloWorldServiceBuilder::new()                            │
│      .say_hello(handler)                                    │
│      .build() ──────────────────────┐                       │
│                                     │                       │
│  UserServiceBuilder::new()          │                       │
│      .get_user(handler)             │   bare Router<()>     │
│      .build() ──────────────────────┤                       │
│                                     │                       │
└─────────────────────────────────────┼───────────────────────┘
                                      ▼
┌─────────────────────────────────────────────────────────────┐
│  MakeServiceBuilder (library)                               │
│                                                             │
│  MakeServiceBuilder::new()                                  │
│      .add_router(hello_router)    ◄─── bare routers in      │
│      .add_router(user_router)                               │
│      .message_limits(...)         ◄─── infrastructure config│
│      .require_protocol_header(...)                          │
│      .build()                     ◄─── applies ConnectLayer │
│                                                             │
└─────────────────────────────────────────────────────────────┘
                                      ▼
                              Final service ready to serve
```

### Separation of Concerns

| Concern | Generated Builder | MakeServiceBuilder |
|---------|-------------------|-------------------|
| Handler registration | ✓ | |
| Per-method routing | ✓ | |
| State application | ✓ | |
| Multi-service composition | | ✓ |
| ConnectLayer application | | ✓ |
| Message limits config | | ✓ |
| Protocol header validation | | ✓ |
| gRPC service integration | | ✓ |

## Design Decisions

- **Newtype wrappers**: Type-safe request/response/streaming distinction
- **Axum integration**: Leverages existing extractor ecosystem
- **Optional Tonic feature**: Single-port gRPC + Connect serving
- **Build-time codegen**: Type-safe service builders with IDE support
- **Two-tier builders**: Generated builders handle service-specific routing; `MakeServiceBuilder` handles cross-cutting concerns
