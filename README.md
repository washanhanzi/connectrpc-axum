# ConnectRPC Axum

[![Crates.io](https://img.shields.io/crates/v/connectrpc-axum.svg)](https://crates.io/crates/connectrpc-axum)
[![Documentation](https://docs.rs/connectrpc-axum/badge.svg)](https://docs.rs/connectrpc-axum)
[![License](https://img.shields.io/crates/l/connectrpc-axum.svg)](LICENSE)

A Rust library that brings [ConnectRPC](https://connectrpc.com/) protocol support to the [Axum](https://github.com/tokio-rs/axum) web framework, with optional [Tonic](https://github.com/hyperium/tonic) integration for serving gRPC on the same port.

> **Status**: Under active development. Not recommended for production use yet.

## What This Library Does

- **Axum Compatibility**: Use Axum's full ecosystem (extractors, middleware, state, layers) with ConnectRPC handlers
- **ConnectRPC Protocol**: Native handling of Connect protocol requests with automatic JSON/Protobuf encoding
- **gRPC via Tonic**: Optional feature to serve both Connect and gRPC clients from a single port

| Protocol | Support |
|----------|---------|
| Connect (JSON/Proto) | Native |
| gRPC | Via Tonic integration |
| gRPC-Web | Via tonic-web layer |

## Features

- Type-safe handlers generated from Protocol Buffers
- All Axum extractors work before `ConnectRequest<T>`
- Unary and server streaming for Connect protocol
- Full streaming (including bidi) via Tonic integration
- Automatic content negotiation (JSON/binary protobuf)

## Development

### Reporting Issues

Use the `submit-issue` subcommand to report bugs or request features:

```bash
claude /submit-issue
```

If not using Claude Code, see [`.claude/skills/submit-issue/SKILL.md`](.claude/skills/submit-issue/SKILL.md) for the workflow.

### Running Tests

Use the `test` subcommand to run unit and integration tests:

```bash
claude /test
```

If not using Claude Code, see [`.claude/skills/test.md`](.claude/skills/test.md) for instructions.

### Architecture

See [`.claude/architecture.md`](.claude/architecture.md) for detailed documentation on the project structure, core modules, and design decisions.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum = "*"
axum = "0.8"
prost = "0.14"
pbjson = "0.8"
tokio = { version = "1", features = ["full"] }

[build-dependencies]
connectrpc-axum-build = "*"
```

For gRPC support, add the tonic feature:

```toml
[dependencies]
connectrpc-axum = { version = "*", features = ["tonic"] }
tonic = "0.14"

[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

## Usage Guide

### 1. Define Your Proto File

Create `proto/hello.proto`:

```protobuf
syntax = "proto3";

package hello;

service HelloWorldService {
  rpc SayHello(HelloRequest) returns (HelloResponse) {}
  rpc SayHelloStream(HelloRequest) returns (stream HelloResponse) {}
}

message HelloRequest {
  optional string name = 1;
}

message HelloResponse {
  string message = 1;
}
```

### 2. Configure Code Generation

Create `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect-only
    connectrpc_axum_build::compile_dir("proto").compile()?;

    // OR: Connect + Tonic (requires "tonic" feature)
    // connectrpc_axum_build::compile_dir("proto").with_tonic().compile()?;

    Ok(())
}
```

### 3. Include Generated Code

In `src/lib.rs`:

```rust
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
pub use pb::*;
```

### ConnectRPC Only Example

Use the generated service builder with any Axum extractors:

```rust
use axum::{extract::State, Router};
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

// Handler with state extractor
async fn say_hello(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

// Server streaming handler
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<StreamBody<HelloResponse>, ConnectError> {
    let stream = async_stream::stream! {
        for i in 0..5 {
            yield Ok(HelloResponse {
                message: format!("Hello #{}, {}!", i, req.name.clone().unwrap_or_default()),
            });
        }
    };
    Ok(StreamBody::new(stream))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the service router (bare router without middleware)
    let hello_router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .say_hello_stream(say_hello_stream)
        .with_state(AppState::default())
        .build();

    // Use MakeServiceBuilder to apply ConnectLayer and configure options
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_router)
        // .message_limits(connectrpc_axum::MessageLimits::new(16 * 1024 * 1024))
        // .require_protocol_header(true)
        .build();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

### gRPC + Connect Example (Tonic Integration)

Serve both Connect and gRPC clients on the same port:

```rust
use axum::extract::State;
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

// Tonic-compatible handlers only allow:
// - (ConnectRequest<Req>)
// - (State<S>, ConnectRequest<Req>)
async fn say_hello(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build both Connect router and Tonic service
    let (router, svc) = helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
        .say_hello(say_hello)
        .with_state(AppState::default())
        .build();

    // Create gRPC server
    let grpc = hello_world_service_server::HelloWorldServiceServer::new(svc);

    // Use MakeServiceBuilder to apply ConnectLayer and combine with gRPC
    // Routes by Content-Type:
    // - application/grpc* -> Tonic gRPC server
    // - Otherwise -> Axum routes (Connect protocol)
    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .add_grpc_service(grpc)
        .build();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
    Ok(())
}
```

## Migration Guide

### From Axum

If you have an existing Axum application and want to add ConnectRPC endpoints, you can merge routers to keep your existing routes alongside new ConnectRPC services:

**Before (Plain Axum)**:

```rust
use axum::{extract::State, Json, Router, routing::get};

#[derive(Clone)]
struct AppState;

async fn get_user(
    State(_s): State<AppState>,
    Json(req): Json<GetUserRequest>,
) -> Json<GetUserResponse> {
    Json(GetUserResponse {
        name: format!("User {}", req.id),
    })
}

let router = Router::new()
    .route("/getUser", get(get_user))
    .with_state(AppState);
```

**After (Keep existing routes + add ConnectRPC)**:

```rust
use axum::{extract::State, Router, routing::post};
use connectrpc_axum::prelude::*;

#[derive(Clone)]
struct AppState;

// Single handler works for both routes (ConnectRPC supports JSON encoding)
async fn get_user(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<user::v1::GetUserRequest>,
) -> Result<ConnectResponse<user::v1::GetUserResponse>, ConnectError> {
    Ok(ConnectResponse(user::v1::GetUserResponse {
        name: format!("User {}", req.id),
    }))
}

// Build ConnectRPC routes
let connect_router = userservice::UserServiceBuilder::new()
    .get_user(get_user)
    .with_state(AppState)
    .build();

// Merge with existing route path for backwards compatibility
let merged_router = Router::new()
    .route("/getUser", post(get_user))  // Keep legacy path: /getUser
    .merge(connect_router)               // Add ConnectRPC path: /user.v1.UserService/GetUser
    .with_state(AppState);

// Use MakeServiceBuilder to apply ConnectLayer
let app = connectrpc_axum::MakeServiceBuilder::new()
    .add_router(merged_router)
    .build();
```

This serves both paths with the same handler (both use JSON):
- `POST /getUser` - Legacy REST endpoint
- `POST /user.v1.UserService/GetUser` - ConnectRPC endpoint

**Key differences**:
- Define message types in `.proto` files instead of Rust structs
- Use `ConnectRequest<T>` instead of `Json<T>` for request body
- Use `ConnectResponse<T>` instead of `Json<T>` for response
- Return `Result<_, ConnectError>` for proper error handling
- Use generated service builders for type-safe routing
- Use `MakeServiceBuilder` to apply `ConnectLayer` middleware
- ConnectRPC routes are `/<package>.<Service>/<Method>`
- Use `Router::merge()` to combine existing and ConnectRPC routes

## Examples

See the [connectrpc-axum-examples](./connectrpc-axum-examples) directory for complete working examples:

| Example | Description |
|---------|-------------|
| `connect-unary` | Pure Connect unary RPC |
| `connect-server-stream` | Pure Connect server streaming |
| `tonic-unary` | Connect + gRPC unary (dual protocol) |
| `tonic-server-stream` | Connect + gRPC streaming (dual protocol) |
| `tonic-bidi-stream` | Bidirectional streaming (gRPC only) |
| `grpc-web` | gRPC-Web browser support |

Run an example:

```bash
cd connectrpc-axum-examples
cargo run --bin connect-unary
```

## Protocol Support

| Feature | Connect Protocol | gRPC (via Tonic) |
|---------|-----------------|------------------|
| Unary RPC | Yes | Yes |
| Server Streaming | Yes | Yes |
| Client Streaming | No | Yes |
| Bidirectional Streaming | No | Yes |
| JSON Encoding | Yes | No |
| Binary Protobuf | Yes | Yes |
| HTTP/1.1 | Yes | No |
| HTTP/2 | Yes | Yes |

## Acknowledgments

This project started as a fork of [AThilenius/axum-connect](https://github.com/AThilenius/axum-connect). If you require only ConnectRPC support, you may also want to check it out.

## Learn More

- [ConnectRPC Protocol](https://connectrpc.com/docs/protocol/)
- [Axum Documentation](https://docs.rs/axum/)
- [Tonic gRPC](https://docs.rs/tonic/)
- [Protocol Buffers](https://protobuf.dev/)

## License

MIT License - see [LICENSE](LICENSE) for details.
