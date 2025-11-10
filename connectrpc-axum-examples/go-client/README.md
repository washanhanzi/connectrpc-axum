# Connect Protocol Streaming Verification

This directory contains both a Go client and a reference Go server for testing Connect protocol streaming implementations.

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

## Components

### 1. Test Client (`cmd/client/`)

- **Raw HTTP Protocol Testing**: Inspects the actual binary frames sent by the server
- **Connect Client Testing**: Uses the official Connect-Go client library
- **Protocol Compliance Verification**: Checks for:
  - Proper frame structure `[flags:1][length:4][payload:N]`
  - EndStreamResponse presence and format
  - Error handling format
  - Metadata/trailers

### 2. Reference Go Server (`cmd/server/`)

A minimal Connect server implementation that only implements the `SayHelloStream` handler. This is useful for:
- Comparing behavior with the Rust connectrpc-axum implementation
- Verifying correct Connect protocol implementation
- Testing the Go client against a known-good server

## Prerequisites

- Go 1.21 or later
- Buf CLI for generating protobuf code
- cargo-make (optional, for task automation)

## Setup

### Option 1: Using cargo-make (Recommended)

From the parent directory (`connectrpc-axum-examples`):

```bash
# One-time setup - generates code and builds everything
cargo make setup

# Or step by step:
cargo make go-generate    # Generate protobuf code
cargo make go-build       # Build the test client
```

### Option 2: Manual setup

1. Install Buf CLI:
```bash
# macOS
brew install bufbuild/buf/buf

# Or using Go
go install github.com/bufbuild/buf/cmd/buf@latest
```

2. Generate the code:
```bash
buf generate proto --template buf.gen.yaml -o .
```

3. Build:
```bash
go mod download
go build -o go-client ./cmd/client
go build -o go-server ./cmd/server
```

## Usage

### Testing the Rust Server (connectrpc-axum)

#### Quick Start with cargo-make (Recommended)

From the parent directory (`connectrpc-axum-examples`):

```bash
# Terminal 1: Start the Rust server
cargo make run-connect-only

# Terminal 2: Run the test client
cargo make go-run
```

#### Manual Usage

1. **Start the connectrpc-axum server** (in a separate terminal):
```bash
cd ..
cargo run --bin connect-only
```

Wait for the server to start (you should see "listening on http://0.0.0.0:3000").

2. **Run the test client**:
```bash
# Using the pre-built binary:
./go-client

# Or run directly:
go run ./cmd/client

# Or build first:
go build -o go-client ./cmd/client
./go-client
```

### Testing the Go Reference Server

To test against the Go reference implementation:

1. **Start the Go server** (in a separate terminal):
```bash
# From the go-client directory:
go run ./cmd/server
# Or use the pre-built binary:
./go-server
# Or build it first:
go build -o go-server ./cmd/server
./go-server
```

The server will start on port 3001.

2. **Update the client to point to the Go server**:
Edit `cmd/client/main.go` and change the `serverURL` constant:
```go
const serverURL = "http://localhost:3001"
```

3. **Run the test client**:
```bash
go run ./cmd/client
# or
./go-client
```

### Comparing Implementations

You can run both servers side-by-side on different ports:
- Rust server (connectrpc-axum): `http://localhost:3000`
- Go server (reference): `http://localhost:3001`

Then run the client against each to compare behavior.

### Testing Different Server Implementations

With cargo-make (from parent directory):
```bash
cargo make run-connect-tonic         # Tonic integration
cargo make run-connect-tonic-bidi    # Bidirectional streaming
```

Or manually:
```bash
cd ..
cargo run --bin connect-only                # Pure Connect implementation
cargo run --bin connect-tonic              # Tonic integration
cargo run --bin connect-tonic-bidi-stream  # Bidirectional streaming
```

## What the Tests Check

### Raw HTTP Protocol Test

This test makes a direct HTTP request and manually parses the streaming response frames. It verifies:

- HTTP response status (should be 200 OK)
- Content-Type header (should be `application/connect+json`)
- Frame structure:
  - Flag byte (bit 0 = compressed, bit 1 = EndStream)
  - Length encoding (4-byte big-endian)
  - Payload format (JSON)
- EndStreamResponse presence (critical!)

### Connect Client Test

Uses the official Connect-Go client to:
- Verify interoperability with standard Connect clients
- Test message parsing and streaming semantics
- Check error handling

## Expected Output for Compliant Server

```
🔬 RAW HTTP PROTOCOL TEST
================================================================================

📥 RESPONSE HEADERS:
  Status: 200 OK
  Content-Type: application/connect+json

📦 RESPONSE FRAMES:

  Frame #1:
    Flags: 0b00000000 (0x00)
    - Compressed: false
    - EndStream: false
    Length: XX bytes
    Payload (JSON):
    { "message": "..." }

  Frame #2:
    ...

  Frame #N (FINAL):
    Flags: 0b00000010 (0x02)  ← EndStream flag MUST be set!
    - Compressed: false
    - EndStream: true
    Length: XX bytes
    Payload (JSON):
    {}  ← Empty for success, or {"error": {...}, "metadata": {...}} for errors
    ✅ EndStream flag detected - stream should end

  Total frames received: N
```

## Common Issues Detected

### ❌ Missing EndStreamResponse

If you see:
```
⚠️  WARNING: No explicit EndStreamResponse was detected!
```

This means the server is ending the stream without sending a proper EndStreamResponse frame with the EndStream flag set. This violates the Connect protocol.

### ❌ Wrong Error Format

Errors should be wrapped in an EndStreamResponse:
```json
{
  "error": {
    "code": "unknown",
    "message": "error message",
    "details": [
      {
        "type": "type.googleapis.com/ErrorType",
        "value": "base64-encoded-data",
        "debug": {...}
      }
    ]
  },
  "metadata": {
    "header-name": ["value"]
  }
}
```

### ❌ Missing Metadata

Trailing metadata should be included in the final EndStreamResponse.

## References

- [Connect Protocol Specification](https://connectrpc.com/docs/protocol/)
- [Connect Streaming](https://connectrpc.com/docs/go/streaming/)
- [Connect-Go Documentation](https://connectrpc.com/docs/go/getting-started/)
