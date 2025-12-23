# ConnectRPC Axum Examples

This directory contains examples demonstrating the `connectrpc-axum` library with various configurations, plus a Go client for protocol verification.

## What is ConnectRPC Axum?

`connectrpc-axum` is a Rust library that brings the [ConnectRPC protocol](https://connectrpc.com/) to the Axum web framework. It provides:

- **Type-safe RPC handlers** using generated code from `.proto` files
- **Full Axum integration** - use extractors, middleware, and state management
- **Optional Tonic compatibility** - run Connect and gRPC services side-by-side
- **All streaming types** - unary, server streaming, client streaming, and bidirectional
- **Automatic JSON/binary serialization** using pbjson

Unlike pure Tonic, ConnectRPC Axum gives you the full power of Axum's ecosystem while maintaining protocol compatibility with gRPC and Connect clients.

## Examples Overview

| Example | Protocol Support | Features |
|---------|------------------|----------|
| **connect-unary** | Connect | Basic unary RPC |
| **connect-server-stream** | Connect | Server streaming |
| **tonic-unary** | Connect + gRPC | Dual-protocol unary |
| **tonic-server-stream** | Connect + gRPC | Dual-protocol streaming |
| **tonic-bidi-stream** | gRPC | Bidirectional streaming |
| **grpc-web** | gRPC-Web | Browser-compatible gRPC |

## Quick Start

### Prerequisites

1. **Rust**: Install from [rustup.rs](https://rustup.rs/)
2. **cargo-make**: Task runner (recommended)
   ```bash
   cargo install cargo-make
   ```
3. **Go 1.21+**: For the test client
4. **Buf CLI**: For generating protobuf code
   ```bash
   brew install bufbuild/buf/buf
   ```

### Setup

```bash
cargo make setup
```

### Running Examples

```bash
# Terminal 1: Start a server
cargo make run-tonic-unary

# Terminal 2: Test with Go client
cargo make go-test-unary          # Connect protocol
cargo make go-test-unary-grpc     # gRPC protocol
```

## Directory Structure

```
connectrpc-axum-examples/
├── proto/                         # Protocol Buffer definitions
│   ├── hello.proto                # HelloWorldService
│   └── echo.proto                 # EchoService (with bidi)
├── src/bin/                       # Rust server examples
│   ├── connect-unary.rs           # Example 1: Pure Connect unary
│   ├── connect-server-stream.rs   # Example 2: Pure Connect streaming
│   ├── tonic-unary.rs             # Example 3: Connect + gRPC unary
│   ├── tonic-server-stream.rs     # Example 4: Connect + gRPC streaming
│   ├── tonic-bidi-stream.rs       # Example 5: gRPC bidi streaming
│   └── grpc-web.rs                # Example 6: gRPC-Web
└── go-client/                     # Go test client
    └── cmd/client/main.go         # Flag-based test runner
```

## Example Details

### Example 1: connect-unary

**Pure ConnectRPC unary** - Simplest example with no gRPC.

```bash
cargo make run-connect-unary
# Test: cargo make go-test-unary
```

Features:
- Single request, single response
- Stateless handler
- No Tonic dependency

### Example 2: connect-server-stream

**Pure ConnectRPC streaming** - Server streams multiple responses.

```bash
cargo make run-connect-server-stream
# Test: cargo make go-test-server-stream
```

Features:
- Uses `async_stream::stream!` macro
- Returns `StreamBody<impl Stream<...>>`
- No Tonic dependency

### Example 3: tonic-unary

**Dual-protocol unary** - Same handler serves Connect and gRPC.

```bash
cargo make run-tonic-unary
# Test: cargo make go-test-unary && cargo make go-test-unary-grpc
```

Features:
- Uses `TonicCompatibleBuilder`
- Single handler for both protocols
- Shared application state
- Routes by Content-Type

### Example 4: tonic-server-stream

**Dual-protocol streaming** - Server streaming for both protocols.

```bash
cargo make run-tonic-server-stream
# Test: cargo make go-test-server-stream && cargo make go-test-server-stream-grpc
```

Features:
- Same streaming handler serves Connect and gRPC
- `MakeServiceBuilder` combines routers

### Example 5: tonic-bidi-stream

**Bidirectional streaming** - gRPC-only feature.

```bash
cargo make run-tonic-bidi-stream
# Test: cargo make go-test-bidi-stream
```

Features:
- Custom Tonic `impl EchoService`
- `tonic::Streaming<Request>` input
- gRPC-only (Connect protocol doesn't support bidi)

### Example 6: grpc-web

**gRPC-Web support** - Browser-compatible gRPC via tonic-web.

```bash
cargo make run-grpc-web
# Test: cargo make go-test-grpc-web
```

Features:
- Uses `tonic_web::GrpcWebLayer`
- HTTP/1.1 compatible for browsers
- Supports both gRPC and gRPC-Web

## Go Client Usage

The Go client supports multiple protocols and test modes:

```bash
# Basic usage
go run ./cmd/client [flags] <command>

# Commands:
#   unary          Test unary RPC
#   server-stream  Test server streaming
#   bidi-stream    Test bidi streaming (gRPC only)
#   grpc-web       Test gRPC-Web protocol
#   all            Run all applicable tests

# Flags:
#   -server    Server URL (default: http://localhost:3000)
#   -protocol  Protocol: connect, grpc (default: connect)
#   -verbose   Verbose output with raw frames
```

### Quick Test Commands

```bash
# Test unary with Connect
cargo make go-test-unary

# Test unary with gRPC
cargo make go-test-unary-grpc

# Test server streaming with Connect
cargo make go-test-server-stream

# Test server streaming with gRPC
cargo make go-test-server-stream-grpc

# Test bidirectional streaming (gRPC only)
cargo make go-test-bidi-stream

# Test gRPC-Web
cargo make go-test-grpc-web

# Run all tests with Connect
cargo make go-test-all

# Run all tests with gRPC
cargo make go-test-all-grpc
```

## Feature Matrix

| Example | Connect | gRPC | gRPC-Web | Unary | Server Stream | Bidi |
|---------|---------|------|----------|-------|---------------|------|
| connect-unary | Y | - | - | Y | - | - |
| connect-server-stream | Y | - | - | - | Y | - |
| tonic-unary | Y | Y | - | Y | - | - |
| tonic-server-stream | Y | Y | - | - | Y | - |
| tonic-bidi-stream | Y | Y | - | Y | Y | Y |
| grpc-web | - | Y | Y | Y | Y | - |

## Protocol Definitions

### hello.proto

```protobuf
service HelloWorldService {
  rpc SayHello(HelloRequest) returns (HelloResponse);
  rpc SayHelloStream(HelloRequest) returns (stream HelloResponse);
}
```

### echo.proto

```protobuf
service EchoService {
  rpc Echo(EchoRequest) returns (EchoResponse);
  rpc EchoClientStream(stream EchoRequest) returns (EchoResponse);
  rpc EchoBidiStream(stream EchoRequest) returns (stream EchoResponse);
}
```

## Available Tasks

```bash
cargo make help
```

### Rust Servers
- `run-connect-unary` - Example 1: Pure ConnectRPC unary
- `run-connect-server-stream` - Example 2: Pure ConnectRPC streaming
- `run-tonic-unary` - Example 3: gRPC + Connect unary
- `run-tonic-server-stream` - Example 4: gRPC + Connect streaming
- `run-tonic-bidi-stream` - Example 5: gRPC bidi streaming
- `run-grpc-web` - Example 6: gRPC-Web
- `build-servers` - Build all examples

### Go Tests
- `go-test-unary` - Test unary (Connect)
- `go-test-unary-grpc` - Test unary (gRPC)
- `go-test-server-stream` - Test streaming (Connect)
- `go-test-server-stream-grpc` - Test streaming (gRPC)
- `go-test-bidi-stream` - Test bidi (gRPC only)
- `go-test-grpc-web` - Test gRPC-Web
- `go-test-all` / `go-test-all-grpc` - Run all tests

### Build & Maintenance
- `setup` - Initial one-time setup
- `build-all` - Build everything
- `clean-all` - Clean all artifacts

## Using in Your Project

### Cargo.toml

```toml
[dependencies]
connectrpc-axum = "0.0.7"
axum = "0.8"
prost = "0.14"
pbjson = "0.8"
pbjson-types = "0.8"
serde = { version = "1.0", features = ["derive"] }
futures = "0.3"
tokio-stream = "0.1"
http-body = "1"
tower = "0.5"

# Optional: Tonic/gRPC support
tonic = { version = "0.14", optional = true }
tonic-prost = { version = "0.14", optional = true }

[build-dependencies]
connectrpc-axum-build = "0.0.9"

[features]
tonic = ["connectrpc-axum/tonic", "connectrpc-axum-build/tonic", "dep:tonic", "dep:tonic-prost"]
```

### build.rs

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let builder = connectrpc_axum_build::compile_dir("proto");

    #[cfg(feature = "tonic")]
    let builder = builder.with_tonic();

    builder.compile()?;
    Ok(())
}
```

### src/lib.rs

```rust
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
    include!(concat!(env!("OUT_DIR"), "/echo.rs"));
}
pub use pb::*;
```

## Troubleshooting

### Port Already in Use

```bash
lsof -ti:3000 | xargs kill -9
```

### Go Client Build Errors

```bash
cargo make go-generate
cargo make go-deps
```

### cargo-make Not Found

```bash
cargo install cargo-make
```

## Learn More

- **ConnectRPC Protocol**: https://connectrpc.com/docs/protocol/
- **Axum Framework**: https://docs.rs/axum/
- **Tonic gRPC**: https://docs.rs/tonic/
- **cargo-make**: https://sagiegurari.github.io/cargo-make/
