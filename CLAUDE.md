# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ConnectRPC-Axum is a Rust library that brings the Connect RPC framework to Axum web servers. It consists of three main crates:
- `connectrpc-axum`: Runtime library providing Connect RPC support for Axum
- `connectrpc-axum-build`: Code generation tool for creating routes from `.proto` files
- `connectrpc-axum-examples`: Example application demonstrating usage

## Development Commands

### Building the Project
```bash
# Build all workspace members
cargo build

# Build a specific crate
cargo build -p connectrpc-axum
cargo build -p connectrpc-axum-build
cargo build -p connectrpc-axum-examples

# Build with release optimizations
cargo build --release
```

### Running the Example
```bash
cargo run -p connectrpc-axum-examples
```
The example server runs on `http://127.0.0.1:3030`

### Testing Endpoints
```bash
# Test unary RPC
curl -X POST http://localhost:3030/hello.HelloWorldService/SayHello \
     -H "Content-Type: application/json" \
     -d '{"name":"Axum"}'

# Test streaming RPC  
curl -X POST http://localhost:3030/hello.HelloWorldService/SayHelloStream \
     -H "Content-Type: application/json" \
     -d '{"name":"Stream"}'
```

### Common Cargo Commands
```bash
# Check for compilation errors without building
cargo check

# Format code
cargo fmt

# Run clippy linter
cargo clippy

# Clean build artifacts
cargo clean

# Update dependencies
cargo update
```

## Architecture

### Code Generation Flow
1. Proto files (`.proto`) define service interfaces
2. `build.rs` uses `connectrpc-axum-build` to generate Rust code at compile time
3. Generated code includes a `routes()` function that creates Axum routes with a `Handlers` struct
4. Service implementations are standard Axum handlers using `ConnectRequest` and `ConnectResponse`

### Key Types and Traits

#### Core Types
- `ConnectRequest<T>`: Extractor for request payloads (MUST be the last parameter in handlers)
- `ConnectResponse<T>`: Response wrapper for unary RPCs
- `ConnectStreamResponse<S>`: Response wrapper for streaming RPCs
- `ConnectError`: Error type mapping to Connect protocol errors with proper HTTP status codes

#### Handler System
The library provides a specialized handler system (`connectrpc-axum/src/handler.rs`):
- `ConnectHandler` trait: Core trait for Connect RPC handlers
- `ConnectService`: Tower service wrapper that bridges handlers to Axum routing
- Helper functions for different handler patterns:
  - `simple_connect_handler`: For handlers that only take `ConnectRequest<T>`
  - `stateful_connect_handler`: For handlers with `State<S>` and `ConnectRequest<T>`
  - `extractor_connect_handler`: For handlers with additional Axum extractors

**IMPORTANT**: `ConnectRequest` must always be the last parameter in handler functions because it consumes the request body.

### Handler Pattern Examples
```rust
// Unary handler with state
async fn handler(
    State(state): State<AppState>,
    Query(params): Query<Params>,        // Other extractors come first
    ConnectRequest(req): ConnectRequest<Req>  // ConnectRequest MUST be last
) -> Result<ConnectResponse<Res>, ConnectError>

// Streaming handler
async fn stream_handler(
    ConnectRequest(req): ConnectRequest<Req>
) -> ConnectStreamResponse<impl Stream<Item = Result<Res, ConnectError>>>
```

### Generated Code Structure
The build process generates a module for each service with:
- A `Handlers` struct containing fields for each RPC method
- A `routes()` function that accepts handler instances and state
- Proper routing paths following Connect protocol: `/{package}.{service}/{method}`

### Dependencies
The project uses `connect-core` from the Connect Rust repository for core protocol support. JSON serialization requires adding serde attributes via prost_build configuration in `build.rs`.