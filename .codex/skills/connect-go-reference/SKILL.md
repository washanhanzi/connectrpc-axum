---
name: connect-go-reference
description: Reference the LOCAL connect-go/ directory for ConnectRPC protocol. NEVER use WebFetch/WebSearch for github.com/connectrpc/connect-go - always read local files.
---

# connect-go-reference

Reference the LOCAL `connect-go/` directory for understanding the ConnectRPC protocol.

## CRITICAL: Use Local Files Only

**NEVER fetch from GitHub.** Do not use:
- `WebFetch` with `github.com/connectrpc/connect-go`
- `WebFetch` with `raw.githubusercontent.com/.../connect-go`
- `WebSearch` for "connect-go" implementation details

**ALWAYS use local files:**
- `Read` tool with `connect-go/*.go` paths
- `Grep` tool with `path="connect-go/"`
- `Glob` tool with `path="connect-go/"`

The `connect-go/` directory at the repository root is the authoritative reference.

## Instructions

The `connect-go/` directory at the repository root contains the official Go implementation of the ConnectRPC protocol.

### Setup (one-time only)

If the directory doesn't exist, clone it once:

```bash
git clone https://github.com/connectrpc/connect-go.git connect-go
```

After cloning, ALWAYS use the local files via Read/Grep/Glob tools.

### Key Files Reference

| File | Purpose |
|------|---------|
| `protocol_connect.go` | Connect protocol implementation (unary + streaming) |
| `protocol_grpc.go` | gRPC protocol implementation |
| `protocol.go` | Protocol abstraction and detection |
| `envelope.go` | Frame/envelope encoding (5-byte header) |
| `client.go` | Client-side implementation |
| `handler.go` | Server-side handler implementation |
| `client_stream.go` | Client streaming logic |
| `handler_stream.go` | Server streaming logic |
| `error.go` | Error types and code mappings |
| `codec.go` | Protobuf/JSON codec interfaces |
| `compression.go` | Compression support (gzip, etc.) |
| `duplex_http_call.go` | HTTP call abstraction for streaming |
| `option.go` | Configuration options |

### Usage Patterns

**Protocol detection logic:**
```
Read connect-go/protocol.go and connect-go/protocol_connect.go for Content-Type parsing
```

**Streaming frame format:**
```
Read connect-go/envelope.go for the 5-byte envelope format:
[flags: 1 byte][length: 4 bytes BE][payload]
```

**Error handling:**
```
Read connect-go/error.go for error codes, wire format, and detail encoding
```

**gRPC trailers:**
```
Read connect-go/protocol_grpc.go for grpc-status header and trailer handling
```

### When to Use

- Implementing new protocol features
- Debugging wire format or encoding issues
- Verifying correct behavior against reference
- Understanding edge cases in streaming
- Checking error code mappings

### How to Use (Examples)

```bash
# Search for content-type handling
Grep pattern="Content-Type" path="connect-go/"

# Read specific file
Read file_path="connect-go/protocol_connect.go"

# Find all error-related code
Grep pattern="Code" path="connect-go/error.go"
```

**FORBIDDEN:**
- `WebFetch("https://github.com/connectrpc/connect-go/...")` - NO
- `WebFetch("https://raw.githubusercontent.com/connectrpc/connect-go/...")` - NO
- `WebSearch("connect-go ...")` for implementation details - NO
- Guessing behavior without reading local `connect-go/*.go` files - NO
