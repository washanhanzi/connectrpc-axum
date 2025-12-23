# ConnectRPC Axum Examples - Task Reference

Quick reference for `cargo-make` tasks. Run from the `connectrpc-axum-examples/` directory.

## Quick Start

```bash
cargo make setup                    # One-time setup
cargo make run-tonic-unary          # Terminal 1: Start server
cargo make go-test-unary            # Terminal 2: Test with Connect
cargo make go-test-unary-grpc       # Terminal 2: Test with gRPC
```

## Server Tasks

| Task | Description |
|------|-------------|
| `cargo make run-connect-unary` | Example 1: Pure ConnectRPC unary |
| `cargo make run-connect-server-stream` | Example 2: Pure ConnectRPC streaming |
| `cargo make run-tonic-unary` | Example 3: Connect + gRPC unary |
| `cargo make run-tonic-server-stream` | Example 4: Connect + gRPC streaming |
| `cargo make run-tonic-bidi-stream` | Example 5: gRPC bidi streaming |
| `cargo make run-grpc-web` | Example 6: gRPC-Web |
| `cargo make build-servers` | Build all server binaries |
| `cargo make watch-server` | Auto-restart on code changes |

## Go Client Test Tasks

| Task | Description |
|------|-------------|
| `cargo make go-test-unary` | Test unary RPC (Connect) |
| `cargo make go-test-unary-grpc` | Test unary RPC (gRPC) |
| `cargo make go-test-server-stream` | Test server streaming (Connect) |
| `cargo make go-test-server-stream-grpc` | Test server streaming (gRPC) |
| `cargo make go-test-bidi-stream` | Test bidi streaming (gRPC only) |
| `cargo make go-test-grpc-web` | Test gRPC-Web |
| `cargo make go-test-all` | Run all tests (Connect) |
| `cargo make go-test-all-grpc` | Run all tests (gRPC) |

## Go Client Build Tasks

| Task | Description |
|------|-------------|
| `cargo make go-generate` | Generate protobuf code from .proto files |
| `cargo make go-build` | Build the test client |
| `cargo make go-deps` | Download Go dependencies |
| `cargo make go-clean` | Remove generated files |

## Build & Maintenance

| Task | Description |
|------|-------------|
| `cargo make build-all` | Build all servers + Go client |
| `cargo make clean-all` | Clean all build artifacts |
| `cargo make setup` | Initial setup (run once) |
| `cargo make help` | Show detailed help |

## Common Workflows

### First Time Setup
```bash
cargo make setup
```

### Test Unary RPC (Connect + gRPC)
```bash
# Terminal 1
cargo make run-tonic-unary

# Terminal 2
cargo make go-test-unary          # Connect protocol
cargo make go-test-unary-grpc     # gRPC protocol
```

### Test Server Streaming
```bash
# Terminal 1
cargo make run-tonic-server-stream

# Terminal 2
cargo make go-test-server-stream      # Connect
cargo make go-test-server-stream-grpc # gRPC
```

### Test Bidirectional Streaming (gRPC only)
```bash
# Terminal 1
cargo make run-tonic-bidi-stream

# Terminal 2
cargo make go-test-bidi-stream
```

### Test gRPC-Web
```bash
# Terminal 1
cargo make run-grpc-web

# Terminal 2
cargo make go-test-grpc-web
```

### Development Loop
```bash
# Auto-rebuild and restart on changes
cargo make watch-server

# In another terminal
cargo make go-test-unary
```

### Clean and Rebuild
```bash
cargo make clean-all
cargo make build-all
```

## File Locations

- **Makefile.toml**: Task definitions
- **proto/**: Protocol Buffer definitions
- **src/bin/**: Server implementations
- **go-client/**: Go test client

## Tips

- Use `cargo make --list-all-steps` to see all tasks with categories
- All servers run on port 3000 by default
- Go client uses `-protocol` flag to switch between connect/grpc

## Troubleshooting

**Port in use:**
```bash
lsof -ti:3000 | xargs kill -9
```

**Missing buf CLI:**
```bash
brew install bufbuild/buf/buf
```

**Missing cargo-make:**
```bash
cargo install cargo-make
```

**Go module errors:**
```bash
cargo make go-clean
cargo make go-generate
cargo make go-deps
```
