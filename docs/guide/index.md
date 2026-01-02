# Getting Started

[![connectrpc-axum](https://img.shields.io/crates/v/connectrpc-axum.svg?label=connectrpc-axum)](https://crates.io/crates/connectrpc-axum)

[![connectrpc-axum-build](https://img.shields.io/crates/v/connectrpc-axum-build.svg?label=connectrpc-axum-build)](https://crates.io/crates/connectrpc-axum-build)

::: warning Early Stage
This project is in early development. Use with caution in production environments.
:::

This guide will help you get started with connectrpc-axum.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum = "*"
axum = "0.8"
prost = "0.14"
pbjson = "0.8"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
# If you need stream support
async-stream = "0.3"

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

## Quick Start

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
    connectrpc_axum_build::compile_dir("proto").compile()?;
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

## ConnectRPC Server

Use the generated service builder with any Axum extractors:

```rust
use axum::extract::State;
use connectrpc_axum::prelude::*;
// Import generated types from your crate
use your_crate::{HelloRequest, HelloResponse, helloworldservice};

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
        .build_connect();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, hello_router).await?;
    Ok(())
}
```
