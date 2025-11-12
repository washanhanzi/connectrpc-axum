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

### How It Works

1. **Build Time**: `connectrpc-axum-build` generates Rust code from your `.proto` files
   - Message types using `prost`
   - Service builders with type-safe method registration
   - Optional Tonic server traits for gRPC compatibility

2. **Runtime**: Your handlers receive typed requests and return typed responses
   - Automatic serialization (JSON or binary protobuf)
   - Streaming support using Rust async streams
   - Full access to Axum extractors (State, Headers, Extensions, etc.)

3. **Deployment**: The generated service builder creates an Axum `Router`
   - Can be merged with other Axum routes
   - Supports middleware and layers
   - Works with any Axum-compatible HTTP server

## Directory Structure

```
connectrpc-axum-examples/
├── Makefile.toml        # cargo-make task definitions (recommended)
├── proto/               # Protocol Buffer definitions
│   ├── hello.proto
│   └── echo.proto
├── src/bin/             # Rust server examples
│   ├── connect-only.rs           # Pure Connect implementation
│   ├── connect-tonic.rs          # Connect + Tonic integration
│   └── connect-tonic-bidi-stream.rs  # Bidirectional streaming
└── go-client/           # Go protocol verification client
    ├── main.go
    └── README.md
```

## Quick Start

### Prerequisites

1. **Rust**: Install from [rustup.rs](https://rustup.rs/)
2. **cargo-make**: Task runner (recommended)
   ```bash
   cargo install cargo-make
   ```
3. **Go**: For the test client (optional)
4. **Buf CLI**: For generating protobuf code (if using Go client)
   ```bash
   brew install bufbuild/buf/buf
   ```

### Setup

Run the one-time setup:
```bash
cargo make setup
```

This will:
- Generate Go protobuf code
- Build all Rust servers
- Build the Go test client

### Running Examples

#### Option 1: Using cargo-make (Recommended)

```bash
# Show all available tasks
cargo make help

# Run a server
cargo make run-connect-only
cargo make run-connect-tonic
cargo make run-connect-tonic-bidi

# In another terminal, test with Go client
cargo make go-run
```

#### Option 2: Using cargo directly

```bash
# Run any example
cargo run --bin connect-only
cargo run --bin connect-tonic
cargo run --bin connect-tonic-bidi-stream
```

## Available Examples

### 1. connect-only.rs

**Pure Connect implementation** - Demonstrates the core ConnectRPC functionality.

```bash
cargo make run-connect-only
# or
cargo run --bin connect-only
```

Features:
- Unary RPC: `SayHello`
- Server streaming: `SayHelloStream`
- Pure Connect protocol (no gRPC)
- Shared state with Axum extractors

**Endpoints:**
- `http://localhost:3000/hello.HelloWorldService/SayHello`
- `http://localhost:3000/hello.HelloWorldService/SayHelloStream`

### 2. connect-tonic.rs

**Connect + Tonic integration** - Shows how to run Connect and gRPC services together.

```bash
cargo make run-connect-tonic
# or
cargo run --bin connect-tonic
```

Features:
- HelloWorldService via Connect router
- Custom Tonic implementation
- Mixed service deployment
- Shared application state

### 3. connect-tonic-bidi-stream.rs

**Bidirectional streaming** - Full-featured example with all streaming types.

```bash
cargo make run-connect-tonic-bidi
# or
cargo run --bin connect-tonic-bidi-stream
```

Features:
- Unary RPC
- Server streaming
- Client streaming (gRPC only - not supported by Connect protocol)
- Bidirectional streaming (gRPC only - not supported by Connect protocol)
- Multiple services on one port

**Note:** Client and bidirectional streaming are only available via Tonic/gRPC, as the Connect protocol only supports unary and server-streaming RPCs.

## Key Differences Between Examples

| Feature | connect-only | connect-tonic | connect-tonic-bidi |
|---------|--------------|---------------|-------------------|
| Connect Protocol | ✅ | ✅ | ✅ |
| gRPC Protocol | ❌ | ✅ | ✅ |
| Unary RPC | ✅ | ✅ | ✅ |
| Server Streaming | ✅ | ✅ | ✅ |
| Client Streaming | ❌ | ❌ | ✅ (gRPC only) |
| Bidirectional Streaming | ❌ | ❌ | ✅ (gRPC only) |
| Axum Extractors | ✅ | ✅ | ✅ |
| Shared State | ✅ | ✅ | ✅ |

## Testing

### Protocol Verification with Go Client

The `go-client` directory contains a comprehensive test client that verifies Connect protocol conformance.

```bash
# Build and run the test client
cargo make go-run
```

The client tests:
- Binary frame structure
- EndStreamResponse presence
- Error handling format
- HTTP status codes
- Content-Type headers

See [go-client/README.md](go-client/README.md) for detailed information.

## Available Tasks (cargo-make)

Run `cargo make help` to see all available tasks:

### Rust Servers
- `cargo make run-connect-only` - Run connect-only example
- `cargo make run-connect-tonic` - Run connect-tonic example
- `cargo make run-connect-tonic-bidi` - Run bidi streaming example
- `cargo make build-servers` - Build all servers

### Go Client
- `cargo make go-generate` - Generate protobuf code
- `cargo make go-build` - Build test client
- `cargo make go-run` - Run test client
- `cargo make go-clean` - Clean generated files

### Build & Maintenance
- `cargo make build-all` - Build everything
- `cargo make clean-all` - Clean all artifacts
- `cargo make setup` - Initial setup

### Development
- `cargo make watch-server` - Auto-restart server on changes (requires cargo-watch)

## Protocol Definitions

All examples use the same protobuf definitions in the `proto/` directory:

### hello.proto
- `SayHello`: Unary RPC
- `SayHelloStream`: Server streaming RPC

### echo.proto
- `Echo`: Unary RPC
- `EchoClientStream`: Client streaming RPC
- `EchoBidiStream`: Bidirectional streaming RPC

## Testing Your Changes

After making changes to the library:

1. **Rebuild the examples:**
   ```bash
   cargo make build-servers
   ```

2. **Run a server:**
   ```bash
   cargo make run-connect-only
   ```

3. **Test with Go client (in another terminal):**
   ```bash
   cargo make go-run
   ```

4. **Watch for changes (optional):**
   ```bash
   cargo make watch-server
   ```

## Using Generated Code in Your Own Project

### Code Generation Setup

1. **Add build dependencies** to your `Cargo.toml`:
   ```toml
   [build-dependencies]
   connectrpc-axum-build = { version = "0.0.4", features = ["tonic"] }
   ```

2. **Create `build.rs`** in your project root:
   ```rust
   fn main() -> Result<(), Box<dyn std::error::Error>> {
       connectrpc_axum_build::compile_dir("proto")
           .with_tonic()  // Optional: enable Tonic gRPC support
           .compile()?;
       Ok(())
   }
   ```

3. **Include generated code** in your `src/lib.rs` or `src/main.rs`:
   ```rust
   // You can include generated code in any module
   pub mod pb {
       include!(concat!(env!("OUT_DIR"), "/your_package.rs"));
   }

   // Re-export for convenience (optional)
   pub use pb::*;
   ```

### Required Runtime Dependencies

The generated code requires these dependencies in your `Cargo.toml`:

```toml
[dependencies]
# Core runtime
connectrpc-axum = "0.0.4"
axum = "0.8"

# Message types and serialization
prost = "0.14"
pbjson = "0.8"
pbjson-types = "0.8"
serde = { version = "1.0", features = ["derive"] }

# Async and streaming
futures = "0.3"
tokio-stream = "0.1"

# HTTP
http-body = "1"
tower = "0.5"

# Optional: Tonic support
tonic = { version = "0.14", optional = true }
tonic-prost = { version = "0.14", optional = true }
```

See the [Cargo.toml](./Cargo.toml) in this directory for detailed comments explaining each dependency.

### Module Organization

Generated code uses `super::` to reference types, so you can organize it however you want:

```rust
// Option 1: Module with re-export (like this example)
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
pub use pb::*;

// Option 2: Direct module (no re-export needed)
pub mod hello {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}

// Option 3: Multiple packages in one module
pub mod proto {
    pub mod hello {
        include!(concat!(env!("OUT_DIR"), "/hello.rs"));
    }
    pub mod echo {
        include!(concat!(env!("OUT_DIR"), "/echo.rs"));
    }
}
```

The generated Tonic traits correctly reference types using `super::TypeName`, so you don't need to re-export types at your crate root.

## Troubleshooting

### Port Already in Use

If you get "Address already in use" errors:
```bash
# Find and kill process on port 3000
lsof -ti:3000 | xargs kill -9
```

### Go Client Build Errors

Make sure you've generated the protobuf code:
```bash
cargo make go-generate
```

### cargo-make Not Found

Install it with:
```bash
cargo install cargo-make
```

Or use cargo directly without cargo-make.

### Code Generation Errors

If you encounter errors with generated code:

1. **Clean and rebuild:**
   ```bash
   cargo clean
   cargo build
   ```

2. **Check your proto files** are in the correct directory (default: `proto/`)

3. **Verify build.rs** is in your project root and properly configured

### Type Resolution Errors

If you see errors like "cannot find type `TypeName` in crate root":

- **This should not happen** with the latest version (0.0.4+)
- The generated code now uses `super::` to reference types
- You can include generated code in any module without crate-level re-exports
- If you still see this, ensure you're using the latest version of `connectrpc-axum-build`

## Learn More

### ConnectRPC Axum
- **Repository**: https://github.com/washanhanzi/connectrpc-axum
- **Crate Documentation**: https://docs.rs/connectrpc-axum/

### Related Technologies
- **ConnectRPC Protocol**: https://connectrpc.com/docs/protocol/
- **Axum Framework**: https://docs.rs/axum/
- **Tonic gRPC**: https://docs.rs/tonic/
- **Protocol Buffers**: https://protobuf.dev/
- **pbjson**: https://docs.rs/pbjson/

### Tools
- **cargo-make Documentation**: https://sagiegurari.github.io/cargo-make/
- **Buf CLI**: https://buf.build/docs/
