# ConnectRPC Axum Examples

This directory contains examples demonstrating the `connectrpc-axum` library with various configurations, plus a Go client for protocol verification.

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
- Client streaming
- Bidirectional streaming
- Multiple services on one port

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

## Learn More

- **ConnectRPC Protocol**: https://connectrpc.com/docs/protocol/
- **cargo-make Documentation**: https://sagiegurari.github.io/cargo-make/
- **Axum Framework**: https://docs.rs/axum/
- **Tonic gRPC**: https://docs.rs/tonic/
