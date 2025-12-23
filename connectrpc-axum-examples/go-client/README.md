# Connect Protocol Test Client

This directory contains a Go test client for verifying Connect and gRPC protocol implementations, plus a reference Go server.

## Project Structure

```
go-client/
├── cmd/
│   ├── client/          # Test client tool
│   │   └── main.go
│   └── server/          # Reference Go server
│       └── main.go
├── gen/                 # Generated protobuf code
├── go-client            # Built client binary
├── go-server            # Built server binary
├── go.mod
└── README.md
```

## Client Usage

```bash
go run ./cmd/client [flags] <command>

# Commands:
#   unary          Test unary RPC
#   server-stream  Test server streaming RPC
#   bidi-stream    Test bidirectional streaming (gRPC only)
#   grpc-web       Test gRPC-Web protocol
#   all            Run all applicable tests

# Flags:
#   -server    Server URL (default: http://localhost:3000)
#   -protocol  Protocol: connect, grpc (default: connect)
#   -verbose   Verbose output showing raw frames
```

## Prerequisites

- Go 1.21 or later
- Buf CLI for generating protobuf code
- cargo-make (optional, for task automation)

## Setup

### Using cargo-make (Recommended)

From the parent directory (`connectrpc-axum-examples`):

```bash
cargo make setup           # One-time setup
cargo make go-build        # Build client and server
```

### Manual setup

1. Install Buf CLI:
```bash
brew install bufbuild/buf/buf
```

2. Generate code and build:
```bash
cd ..
buf generate
cd go-client
go mod tidy
go build -o go-client ./cmd/client
go build -o go-server ./cmd/server
```

## Testing Rust Servers

### Quick Start with cargo-make

From the parent directory (`connectrpc-axum-examples`):

```bash
# Terminal 1: Start a Rust server
cargo make run-tonic-unary

# Terminal 2: Run tests
cargo make go-test-unary          # Connect protocol
cargo make go-test-unary-grpc     # gRPC protocol
```

### Available Test Commands

| Command | Description |
|---------|-------------|
| `cargo make go-test-unary` | Test unary (Connect) |
| `cargo make go-test-unary-grpc` | Test unary (gRPC) |
| `cargo make go-test-server-stream` | Test streaming (Connect) |
| `cargo make go-test-server-stream-grpc` | Test streaming (gRPC) |
| `cargo make go-test-bidi-stream` | Test bidi (gRPC only) |
| `cargo make go-test-grpc-web` | Test gRPC-Web |
| `cargo make go-test-all` | All tests (Connect) |
| `cargo make go-test-all-grpc` | All tests (gRPC) |

### Manual Usage

```bash
# Start a Rust server
cd ..
cargo run --bin tonic-unary

# Run tests (in another terminal)
cd go-client
./go-client -protocol connect unary
./go-client -protocol grpc unary
./go-client -protocol connect server-stream
./go-client -protocol grpc bidi-stream
```

## Testing Different Server Examples

| Rust Server | Recommended Tests |
|-------------|-------------------|
| `run-connect-unary` | `go-test-unary` |
| `run-connect-server-stream` | `go-test-server-stream` |
| `run-tonic-unary` | `go-test-unary`, `go-test-unary-grpc` |
| `run-tonic-server-stream` | `go-test-server-stream`, `go-test-server-stream-grpc` |
| `run-tonic-bidi-stream` | `go-test-bidi-stream` |
| `run-grpc-web` | `go-test-grpc-web` |

## Reference Go Server

A minimal Connect server for comparing behavior:

```bash
# Start Go server on port 3001
cargo make go-run-server

# Test against it
./go-client -server http://localhost:3001 -protocol connect unary
```

## What the Tests Check

### Connect Protocol Tests

- HTTP response status (200 OK)
- Content-Type header (`application/connect+json` or `application/connect+proto`)
- Frame structure: `[flags:1][length:4][payload:N]`
- EndStreamResponse presence
- Error handling format

### gRPC Protocol Tests

- HTTP/2 communication
- gRPC status codes
- Streaming semantics
- Bidirectional streaming (not supported by Connect)

### Verbose Mode

Use `-verbose` flag to see raw protocol frames:

```bash
./go-client -verbose -protocol connect server-stream
```

## Expected Output

```
============================================================
UNARY RPC TEST [CONNECT]
============================================================
Response: Hello, Connect Unary Tester!

============================================================
SERVER STREAMING TEST [CONNECT]
============================================================
  [1] Hello, Connect Stream Tester! Starting stream...
  [2] Hobby #1: coding - nice!
  [3] Hobby #2: testing - nice!
  [4] Stream complete. Goodbye, Connect Stream Tester!
Received 4 messages
```

## Troubleshooting

### Connection Refused

Make sure the Rust server is running:
```bash
cargo make run-tonic-unary
```

### Missing Generated Code

Regenerate protobuf code:
```bash
cargo make go-generate
cargo make go-deps
```

### gRPC Tests Failing

Ensure you're using a Tonic-enabled server:
```bash
cargo make run-tonic-unary  # Works with gRPC
# Not: cargo make run-connect-unary  # Connect only
```

## References

- [Connect Protocol Specification](https://connectrpc.com/docs/protocol/)
- [Connect-Go Documentation](https://connectrpc.com/docs/go/getting-started/)
- [gRPC-Go Documentation](https://grpc.io/docs/languages/go/)
