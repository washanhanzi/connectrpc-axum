# ConnectRPC Axum

[![Crates.io](https://img.shields.io/crates/v/connectrpc-axum.svg)](https://crates.io/crates/connectrpc-axum)
[![Documentation](https://docs.rs/connectrpc-axum/badge.svg)](https://docs.rs/connectrpc-axum)
[![License](https://img.shields.io/crates/l/connectrpc-axum.svg)](LICENSE)

A Rust implementation of the [ConnectRPC](https://connectrpc.com/) protocol for the [Axum](https://github.com/tokio-rs/axum) web framework.

## Features

- 🚀 **Native Axum integration** - Use extractors, middleware, state, and layers
- 🔒 **Type-safe** - Generated code from Protocol Buffers with compile-time guarantees
- 🔄 **Streaming support** - Unary, server streaming, client streaming, and bidirectional streaming
- 🔌 **Tonic compatibility** - Optional gRPC interop for running Connect and gRPC on the same port
- 📦 **Automatic serialization** - JSON and binary Protocol Buffers via `pbjson`
- 🎯 **Flexible handlers** - Use any Axum extractors before the request body
- ⚡ **High performance** - Built on Axum's proven stack (hyper, tokio, tower)

> **Status**: Under active development. Not recommended for production use yet.

## Quick Start

### Installation

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

### 1. Code Generation (build.rs)

Add a build script to generate code from your `.proto` files.

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // EITHER: Connect-only
    connectrpc_axum_build::compile_dir("proto").compile()?;

    // OR: Connect + Tonic (enable the "tonic" feature on the build crate)
    // connectrpc_axum_build::compile_dir("proto").with_tonic().compile()?;
    Ok(())
}
```

### 2. Create Your Proto Files

Create a `proto/hello.proto` file:

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

### 3. Include Generated Code

In your `src/lib.rs` or `src/main.rs`:

```rust
// You can include generated code in any module
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}

// Re-export for convenience (optional)
pub use pb::*;
```

### 4. Build a Connect Server

Use any number of `FromRequestParts` extractors first, and end with `ConnectRequest<T>`.

```rust
use axum::{extract::{Query, State}, Router};
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

#[derive(serde::Deserialize)]
struct Pagination { page: usize, per_page: usize }

// Multiple extractors + Connect body
async fn say_hello(
    Query(_p): Query<Pagination>,
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hello, {}!", req.name.unwrap_or_default()) }))
}

// Minimal handler (no state)
async fn say_hello_simple(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hi, {}!", req.name.unwrap_or_default()) }))
}

// Build routes via the generated service builder (no manual paths)
let router = helloworldservice::HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .say_hello_stream(say_hello_simple)
    .with_state(AppState::default())
    .build();

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, tower::make::Shared::new(router)).await?;
```

## Advanced Usage

### Streaming Support

ConnectRPC Axum supports all streaming types:

```rust
use futures::Stream;
use std::pin::Pin;

// Server streaming
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<StreamBody<HelloResponse>, ConnectError> {
    let stream = async_stream::stream! {
        for i in 0..5 {
            yield Ok(HelloResponse {
                message: format!("Hello #{}", i),
            });
        }
    };
    Ok(StreamBody::new(stream))
}
```

**Note**: Client streaming and bidirectional streaming are only supported via the Tonic integration, as the Connect protocol only supports unary and server-streaming RPCs.

### Tonic-Compatible Server (gRPC Interop)

Enable features in `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum = { version = "*", features = ["tonic"] }

[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

build.rs:

```rust
connectrpc_axum_build::compile_dir("proto").with_tonic().compile()?;
```

Use the generated Tonic-compatible builder and single-port dispatcher:

```rust
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

// Tonic-compatible handler signatures (only these two compile):
// 1) (ConnectRequest<Req>)
// 2) (State<S>, ConnectRequest<Req>)
async fn say_hello(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hello, {}!", req.name.unwrap_or_default()) }))
}

let (router, svc) = helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
    .say_hello(say_hello)
    .with_state(AppState::default())
    .build();

let grpc = hello_world_service_server::HelloWorldServiceServer::new(svc);

let dispatch = connectrpc_axum::ContentTypeSwitch::new(grpc, router);
axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
```

**Constraints in Tonic-compatible mode:**
- Allowed handler signatures:
  - `(ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>`
  - `(State<S>, ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>`
- In Connect-only mode, any number of extractors is allowed before `ConnectRequest<Req>`

### Module Organization

Generated code uses `super::` to reference types, giving you flexibility in organization:

```rust
// Option 1: Module with re-export (convenient)
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
pub use pb::*;

// Option 2: Direct module (no re-export needed)
pub mod hello {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}

// Option 3: Multiple packages
pub mod proto {
    pub mod hello {
        include!(concat!(env!("OUT_DIR"), "/hello.rs"));
    }
    pub mod auth {
        include!(concat!(env!("OUT_DIR"), "/auth.rs"));
    }
}
```

The generated Tonic traits correctly reference types using `super::TypeName`, so you don't need to re-export types at your crate root.

## Examples

See the [connectrpc-axum-examples](./connectrpc-axum-examples) directory for complete working examples:

- **[connect-only.rs](./connectrpc-axum-examples/src/bin/connect-only.rs)** - Pure Connect implementation with Axum extractors
- **[connect-tonic.rs](./connectrpc-axum-examples/src/bin/connect-tonic.rs)** - Connect + Tonic integration
- **[connect-tonic-bidi-stream.rs](./connectrpc-axum-examples/src/bin/connect-tonic-bidi-stream.rs)** - Full streaming support

Run an example:
```bash
cd connectrpc-axum-examples
cargo run --bin connect-only
```

See the [examples README](./connectrpc-axum-examples/README.md) for detailed documentation.

## Protocol Support

| Feature | Connect Protocol | gRPC (via Tonic) |
|---------|-----------------|------------------|
| Unary RPC | ✅ | ✅ |
| Server Streaming | ✅ | ✅ |
| Client Streaming | ❌ | ✅ |
| Bidirectional Streaming | ❌ | ✅ |
| JSON Encoding | ✅ | ❌ |
| Binary Protobuf | ✅ | ✅ |
| HTTP/1.1 | ✅ | ❌ |
| HTTP/2 | ✅ | ✅ |

## Why ConnectRPC Axum?

### vs Pure Tonic

- **Full Axum ecosystem**: Use any Axum extractor, middleware, or layer
- **Flexible handler signatures**: Not limited to Tonic's trait methods
- **Better ergonomics**: Less boilerplate, more idiomatic Rust
- **JSON support**: Automatic JSON serialization via pbjson
- **HTTP/1.1 support**: Works with standard HTTP clients

### vs Other RPC Frameworks

- **Type safety**: Compile-time guarantees from Protocol Buffers
- **Standard protocol**: Compatible with ConnectRPC clients in any language
- **Battle-tested stack**: Built on Axum, hyper, and tokio
- **Optional gRPC**: Can run both protocols on the same port

## Troubleshooting

### Type Resolution Errors

If you see errors like "cannot find type `TypeName` in crate root":

- The generated code now uses `super::` to reference types
- You can include generated code in any module without crate-level re-exports

### Code Generation Errors

If protobuf code generation fails:

1. Check that your `.proto` files are in the correct directory (default: `proto/`)
2. Verify `build.rs` is properly configured
3. Run `cargo clean && cargo build` to regenerate

### Streaming Issues

Remember:
- Server streaming works with both Connect and gRPC
- Client/bidirectional streaming requires the Tonic integration
- The Connect protocol only supports unary and server streaming

## Contributing

Contributions are welcome! This project is under active development.

Please see the [examples](./connectrpc-axum-examples) for detailed usage patterns and test your changes against them.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Special thanks to:

- [AThilenius/axum-connect](https://github.com/AThilenius/axum-connect) - Inspiration for the Axum integration approach
- [tokio-rs/axum](https://github.com/tokio-rs/axum) - The amazing web framework this builds on
- [ConnectRPC](https://connectrpc.com/) - The protocol specification
- [hyperium/tonic](https://github.com/hyperium/tonic) - gRPC implementation and interop

## Learn More

- **ConnectRPC Protocol**: https://connectrpc.com/docs/protocol/
- **Axum Documentation**: https://docs.rs/axum/
- **Tonic gRPC**: https://docs.rs/tonic/
- **Protocol Buffers**: https://protobuf.dev/