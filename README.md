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

### Claude Code Slash Commands

This project provides [slash commands](https://docs.anthropic.com/en/docs/claude-code/slash-commands) for common development tasks:

| Command | Description |
|---------|-------------|
| `/submit-issue` | Report bugs, request features, or ask questions |
| `/test` | Run the full test suite |

Usage:

```bash
claude /submit-issue "Description of your issue or feature request"
claude /test
```

If not using Claude Code, see the corresponding skill files in [`.claude/skills/`](.claude/skills/) for instructions.

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

## Usage

### Quick Start

#### 1. Define Your Proto File

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

#### 2. Configure Code Generation

Create `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto").compile()?;
    Ok(())
}
```

#### 3. Include Generated Code

In `src/lib.rs`:

```rust
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
pub use pb::*;
```

### ConnectRPC Server

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
        .build();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

### Enabling Tonic gRPC Support

Serve both Connect and gRPC clients on the same port:

#### 1. Update build.rs

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()  // Enable Tonic gRPC code generation
        .compile()?;
    Ok(())
}
```

#### 2. Use TonicCompatibleBuilder

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

// Build ConnectRPC routes with MakeServiceBuilder (applies ConnectLayer)
let connect_router = connectrpc_axum::MakeServiceBuilder::new()
    .add_router(
        userservice::UserServiceBuilder::new()
            .get_user(get_user)
            .with_state(AppState)
            .build()
    )
    .build();

// Merge with existing HTTP routes
let app = Router::new()
    .route("/getUser", post(get_user))  // Keep legacy path: /getUser
    .merge(connect_router)               // Add ConnectRPC path: /user.v1.UserService/GetUser
    .with_state(AppState);
```

This serves both paths with the same handler (both use JSON):
- `POST /getUser` - Legacy REST endpoint
- `POST /user.v1.UserService/GetUser` - ConnectRPC endpoint

## Configuration

### Build Configuration

#### Prost Configuration

Use `.with_prost_config()` to customize `prost_build::Config`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            // Add custom derives to all generated types
            config.type_attribute(".", "#[derive(Hash)]");

            // Add field attributes
            config.field_attribute("MyMessage.my_field", "#[serde(skip)]");
        })
        .compile()?;
    Ok(())
}
```

#### Compiling Well-Known Types

To use Google's well-known types (like `Timestamp`, `Duration`, `Any`), configure extern paths:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            // Use pbjson_types for well-known types (recommended for JSON support)
            config.extern_path(".google.protobuf", "::pbjson_types");

            // OR use prost_types if you don't need JSON serialization
            // config.extern_path(".google.protobuf", "::prost_types");
        })
        .compile()?;
    Ok(())
}
```

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
pbjson-types = "0.8"  # For JSON-compatible well-known types
# OR
prost-types = "0.14"  # For binary-only well-known types
```

#### Tonic Configuration

When using the `tonic` feature, use `.with_tonic_prost_config()` to customize tonic's code generation:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .with_tonic_prost_config(|builder| {
            builder
                .type_attribute("MyMessage", "#[derive(Hash)]")
                .field_attribute("MyMessage.my_field", "#[serde(skip)]")
        })
        .compile()?;
    Ok(())
}
```

### Timeout Settings

ConnectRPC supports request timeouts via the `Connect-Timeout-Ms` header. This library provides built-in timeout enforcement that returns proper Connect `deadline_exceeded` errors.

#### Server-Side Timeout

Set a maximum timeout for all requests using `MakeServiceBuilder`:

```rust
use std::time::Duration;

let app = connectrpc_axum::MakeServiceBuilder::new()
    .add_router(router)
    .timeout(Duration::from_secs(30))  // Server-side max timeout
    .build();
```

#### How Timeouts Work

| Scenario | Effective Timeout |
|----------|-------------------|
| Client sends `Connect-Timeout-Ms: 5000` | 5 seconds |
| Server sets `.timeout(30s)` | 30 seconds |
| Both set (client: 5s, server: 30s) | 5 seconds (minimum wins) |
| Both set (client: 60s, server: 30s) | 30 seconds (minimum wins) |

> **Note**: Using Axum's `TimeoutLayer` will NOT give you Connect protocol timeout behavior. Always use `.timeout()` on `MakeServiceBuilder` for proper `Connect-Timeout-Ms` header handling.

For a complete example, see [`timeout`](./connectrpc-axum-examples/src/bin/timeout.rs).

## Examples

See the [connectrpc-axum-examples](./connectrpc-axum-examples) directory for complete working examples:

| Example | Description |
|---------|-------------|
| `connect-unary` | Pure Connect unary RPC |
| `connect-server-stream` | Pure Connect server streaming |
| `connect-client-stream` | Pure Connect client streaming |
| `connect-bidi-stream` | Pure Connect bidirectional streaming |
| `tonic-unary` | Connect + gRPC unary (dual protocol) |
| `tonic-server-stream` | Connect + gRPC streaming (dual protocol) |
| `tonic-bidi-stream` | Bidirectional streaming (gRPC only) |
| `grpc-web` | gRPC-Web browser support |
| `timeout` | Connect-Timeout-Ms header handling |
| `protocol-version` | Connect-Protocol-Version header validation |
| `streaming-error-repro` | Streaming error handling demonstration |

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
