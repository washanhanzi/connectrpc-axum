# connect-go-reference

Reference the official connect-go implementation for understanding the ConnectRPC protocol. Use when implementing protocol features, debugging wire format issues, or comparing behavior with the Go reference.

## Instructions

The `connect-go/` directory contains the official Go implementation of the ConnectRPC protocol, cloned from https://github.com/connectrpc/connect-go.git

### Setup

If the directory doesn't exist, clone it:

```bash
git clone https://github.com/connectrpc/connect-go.git connect-go
```

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
