---
name: tonic-client-reference
description: Reference the LOCAL tonic/ directory for gRPC client implementation patterns. NEVER use WebFetch/WebSearch for github.com/hyperium/tonic - always read local files.
---

# tonic-client-reference

Reference the LOCAL `tonic/` directory for understanding gRPC client implementation patterns in Rust.

## CRITICAL: Use Local Files Only

**NEVER fetch from GitHub.** Do not use:
- `WebFetch` with `github.com/hyperium/tonic`
- `WebFetch` with `raw.githubusercontent.com/.../tonic`
- `WebSearch` for "tonic" implementation details

**ALWAYS use local files:**
- `Read` tool with `tonic/**/*.rs` paths
- `Grep` tool with `path="tonic/"`
- `Glob` tool with `path="tonic/"`

The `tonic/` directory at the repository root is the authoritative reference.

## Instructions

The `tonic/` directory at the repository root contains the official Rust gRPC implementation.

### Setup (one-time only)

If the directory doesn't exist, clone it once:

```bash
git clone https://github.com/hyperium/tonic.git tonic
```

After cloning, ALWAYS use the local files via Read/Grep/Glob tools.

### Key Files Reference

| File | Purpose |
|------|---------|
| `tonic/tonic/src/client/grpc.rs` | Main gRPC client implementation |
| `tonic/tonic/src/client/service.rs` | Client service wrapper |
| `tonic/tonic/src/request.rs` | Request wrapper with metadata |
| `tonic/tonic/src/response.rs` | Response wrapper with metadata |
| `tonic/tonic/src/metadata/` | gRPC metadata (headers) handling |
| `tonic/tonic/src/codec/` | Encoding/decoding traits and implementations |
| `tonic/tonic/src/transport/channel/` | HTTP/2 channel and connection management |
| `tonic/tonic/src/transport/channel/service/` | Tower service implementations |
| `tonic/tonic/src/status.rs` | gRPC status codes and error handling |
| `tonic/tonic/src/extensions.rs` | Request/response extensions |
| `tonic/tonic/src/body.rs` | HTTP body handling |

### Client-Specific Files

| File | Purpose |
|------|---------|
| `tonic/tonic/src/transport/channel/endpoint.rs` | Endpoint builder and configuration |
| `tonic/tonic/src/transport/channel/mod.rs` | Channel implementation |
| `tonic/tonic/src/transport/channel/service/reconnect.rs` | Reconnection logic |
| `tonic/tonic/src/transport/channel/service/tls.rs` | TLS configuration |
| `tonic/tonic/src/transport/service/grpc_timeout.rs` | Timeout handling |

### Codec/Streaming Files

| File | Purpose |
|------|---------|
| `tonic/tonic/src/codec/decode.rs` | Streaming decoder implementation |
| `tonic/tonic/src/codec/encode.rs` | Streaming encoder implementation |
| `tonic/tonic/src/codec/compression.rs` | Compression handling |
| `tonic/tonic/src/codec/buffer.rs` | Buffer management for streaming |

### Retry/Backoff Files

| File | Purpose |
|------|---------|
| `tonic/grpc/src/client/name_resolution/backoff.rs` | Exponential backoff implementation |
| `tonic/tonic-types/src/richer_error/std_messages/retry_info.rs` | RetryInfo error detail type |

### Usage Patterns

**Channel configuration:**
```
Read tonic/tonic/src/transport/channel/endpoint.rs for Endpoint and Channel setup
```

**Streaming implementation:**
```
Read tonic/tonic/src/codec/decode.rs and encode.rs for streaming frame handling
```

**Metadata handling:**
```
Read tonic/tonic/src/metadata/map.rs for MetadataMap implementation
```

**Error/Status handling:**
```
Read tonic/tonic/src/status.rs for Status type and code mappings
```

**Interceptors:**
```
Read tonic/tonic/src/service/interceptor.rs for interceptor patterns
```

**Retry/Backoff:**
```
Read tonic/grpc/src/client/name_resolution/backoff.rs for exponential backoff with jitter
```

### When to Use

- Implementing client-side gRPC features
- Understanding streaming patterns
- Debugging connection or channel issues
- Verifying correct metadata handling
- Checking status code mappings
- Understanding Tower service integration

### How to Use (Examples)

```bash
# Search for channel configuration
Grep pattern="Endpoint" path="tonic/tonic/src/transport/"

# Read client implementation
Read file_path="tonic/tonic/src/client/grpc.rs"

# Find streaming-related code
Grep pattern="Streaming" path="tonic/tonic/src/codec/"

# Search for interceptor patterns
Grep pattern="Interceptor" path="tonic/tonic/src/"

# Search for backoff/retry patterns
Grep pattern="backoff" path="tonic/grpc/src/client/"
```

**FORBIDDEN:**
- `WebFetch("https://github.com/hyperium/tonic/...")` - NO
- `WebFetch("https://raw.githubusercontent.com/hyperium/tonic/...")` - NO
- `WebSearch("tonic ...")` for implementation details - NO
- Guessing behavior without reading local `tonic/**/*.rs` files - NO
