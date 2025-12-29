# PR 7 — gRPC-Web Validation and Cleanup

## Problem Summary

The current gRPC-Web support in connectrpc-axum delegates entirely to the `tonic-web` crate via a feature flag. While functional, this approach lacks:
- Native protocol handling for gRPC-Web-specific semantics
- Proper trailer propagation using the gRPC-Web trailer frame format
- Consistent status mapping between Connect and gRPC-Web protocols
- Comprehensive validation of gRPC-Web content types

### Current State

**Implemented:**
- `tonic-web` feature flag integration (`Cargo.toml`)
- `ContentTypeSwitch` dispatcher in `tonic.rs` (lines 1-128)
- Basic content-type prefix detection: `s.starts_with("application/grpc")`
- Example server in `examples/src/bin/grpc-web.rs`

**Missing/Incomplete:**
- Native gRPC-Web trailer frame encoding (flag 0x80)
- `application/grpc-web+json` codec validation
- Explicit `application/grpc-web-text` handling
- Protocol-specific error response formatting
- Integration with Connect's error handling

## gRPC-Web Protocol Requirements

### Content-Types

From `connect-go/protocol_grpc.go`:

| Content-Type | Codec | Description |
|--------------|-------|-------------|
| `application/grpc-web` | proto (default) | Binary protobuf |
| `application/grpc-web+proto` | proto | Explicit protobuf |
| `application/grpc-web+json` | json | JSON encoding |
| `application/grpc-web-text` | proto (base64) | Base64-encoded binary for browsers without binary support |

### Trailer Frame Format

**Critical difference from HTTP/2 trailers:**

Standard gRPC uses HTTP/2 trailers (`grpc-status`, `grpc-message` headers in trailer section). gRPC-Web cannot rely on HTTP/2 trailers because it must work over HTTP/1.1.

**gRPC-Web solution:** Encode trailers as a special message frame in the response body.

```
Frame format:
┌──────────────────┬────────────────────┬─────────────────────────────┐
│ Flag (1 byte)    │ Length (4 bytes)   │ Payload                     │
├──────────────────┼────────────────────┼─────────────────────────────┤
│ 0x00             │ message length     │ Message data                │
│ 0x80 (trailer)   │ headers length     │ HTTP headers as text        │
└──────────────────┴────────────────────┴─────────────────────────────┘
```

**Trailer payload format:** HTTP headers as CRLF-delimited text
```
grpc-status: 0\r\n
grpc-message: OK\r\n
```

From connect-go (`protocol_grpc.go` line 42):
```go
grpcFlagEnvelopeTrailer = 0b10000000  // 0x80 = 128
```

### Status Mapping

gRPC status codes must be included in the trailer frame:

| gRPC Status | Code | Connect Equivalent |
|-------------|------|-------------------|
| OK | 0 | - |
| Cancelled | 1 | cancelled |
| Unknown | 2 | unknown |
| InvalidArgument | 3 | invalid_argument |
| DeadlineExceeded | 4 | deadline_exceeded |
| NotFound | 5 | not_found |
| AlreadyExists | 6 | already_exists |
| PermissionDenied | 7 | permission_denied |
| ResourceExhausted | 8 | resource_exhausted |
| FailedPrecondition | 9 | failed_precondition |
| Aborted | 10 | aborted |
| OutOfRange | 11 | out_of_range |
| Unimplemented | 12 | unimplemented |
| Internal | 13 | internal |
| Unavailable | 14 | unavailable |
| DataLoss | 15 | data_loss |
| Unauthenticated | 16 | unauthenticated |

## Proposed Changes

### 1. Add gRPC-Web Protocol Variant

**File:** `context/protocol.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RequestProtocol {
    ConnectUnaryJson,
    ConnectUnaryProto,
    ConnectStreamJson,
    ConnectStreamProto,
    // New variants for explicit gRPC-Web handling (optional)
    GrpcWebProto,
    GrpcWebJson,
    GrpcWebText,
}
```

### 2. Content-Type Validation

**File:** `layer/content_type.rs`

```rust
pub fn validate_grpc_web_content_type(content_type: &str) -> Option<&str> {
    // Returns codec name if valid, None if invalid
    if content_type == "application/grpc-web" ||
       content_type == "application/grpc-web+proto" {
        Some("proto")
    } else if content_type == "application/grpc-web+json" {
        Some("json")
    } else if content_type == "application/grpc-web-text" ||
              content_type == "application/grpc-web-text+proto" {
        Some("proto")  // base64-encoded proto
    } else {
        None
    }
}
```

### 3. Trailer Frame Encoding for gRPC-Web

**File:** `message/response.rs` (lines 196-302)

Add gRPC-Web trailer encoding:

```rust
fn encode_grpc_web_trailers(status: u32, message: &str, metadata: &HeaderMap) -> Bytes {
    let mut trailer_text = format!("grpc-status: {}\r\n", status);
    if !message.is_empty() {
        trailer_text.push_str(&format!("grpc-message: {}\r\n", message));
    }
    for (key, value) in metadata.iter() {
        if let Ok(v) = value.to_str() {
            trailer_text.push_str(&format!("{}: {}\r\n", key.as_str(), v));
        }
    }

    let trailer_bytes = trailer_text.as_bytes();
    let mut frame = BytesMut::with_capacity(5 + trailer_bytes.len());
    frame.put_u8(0x80); // Trailer flag
    frame.put_u32(trailer_bytes.len() as u32);
    frame.put_slice(trailer_bytes);
    frame.freeze()
}
```

### 4. Enhanced ContentTypeSwitch

**File:** `tonic.rs` (lines 16-34)

```rust
fn is_grpc_request(content_type: &str) -> Option<GrpcVariant> {
    if content_type.starts_with("application/grpc-web") {
        Some(GrpcVariant::GrpcWeb)
    } else if content_type.starts_with("application/grpc") {
        Some(GrpcVariant::Grpc)
    } else {
        None
    }
}

enum GrpcVariant {
    Grpc,      // Native gRPC (HTTP/2)
    GrpcWeb,   // gRPC-Web (HTTP/1.1 compatible)
}
```

### 5. Error Response for gRPC-Web

**File:** `error.rs`

Add gRPC-Web error response formatting:

```rust
pub fn into_grpc_web_error_response(self) -> Response<BoxBody> {
    let grpc_status = self.code.to_grpc_code();
    let message = self.message.clone();

    // For unary: return trailers-only response
    let trailer_frame = encode_grpc_web_trailers(grpc_status, &message, &HeaderMap::new());

    Response::builder()
        .status(StatusCode::OK)  // gRPC-Web always returns 200
        .header("content-type", "application/grpc-web+proto")
        .header("grpc-status", grpc_status.to_string())
        .header("grpc-message", &message)
        .body(Full::new(trailer_frame).map_err(|_| unreachable!()).boxed())
        .unwrap()
}
```

## Files to Modify

| File | Lines | Changes |
|------|-------|---------|
| `context/protocol.rs` | 1-62 | Add gRPC-Web protocol variants (optional) |
| `layer/content_type.rs` | 1-70 | Add gRPC-Web validation functions |
| `message/response.rs` | 196-302 | Add trailer frame encoding (flag 0x80) |
| `error.rs` | 174-267 | Add gRPC-Web error response formatting |
| `tonic.rs` | 16-34 | Enhanced content-type routing |

## Testing Requirements

### Test 1: Content-Type Validation
```rust
#[test]
fn test_grpc_web_content_types() {
    assert!(validate_grpc_web_content_type("application/grpc-web+proto").is_some());
    assert!(validate_grpc_web_content_type("application/grpc-web+json").is_some());
    assert!(validate_grpc_web_content_type("application/grpc-web-text").is_some());
    assert!(validate_grpc_web_content_type("application/grpc-web+xml").is_none());
}
```

### Test 2: Unary RPC
```rust
#[tokio::test]
async fn test_grpc_web_unary() {
    // Send POST with application/grpc-web+proto
    // Verify response has correct content-type
    // Verify trailer frame with grpc-status: 0
}
```

### Test 3: Server Streaming
```rust
#[tokio::test]
async fn test_grpc_web_server_streaming() {
    // Verify message frames (flag 0x00)
    // Verify final trailer frame (flag 0x80)
    // Parse trailer frame and verify grpc-status header
}
```

### Test 4: Error Handling
```rust
#[tokio::test]
async fn test_grpc_web_error() {
    // Trigger error
    // Verify HTTP 200 status (gRPC-Web requirement)
    // Verify grpc-status header
    // Verify trailer frame contains status
}
```

### Test 5: Protocol Routing
```rust
#[tokio::test]
async fn test_protocol_routing() {
    // Test same endpoint with different content-types:
    // - application/connect+proto → Connect handler
    // - application/grpc → Tonic handler
    // - application/grpc-web+proto → Tonic with gRPC-Web
    // Verify no interference between protocols
}
```

### Test 6: Trailer Propagation
```rust
#[tokio::test]
async fn test_grpc_web_custom_trailers() {
    // Handler adds custom trailer metadata
    // Verify trailers appear in gRPC-Web trailer frame
    // Verify format: "key: value\r\n"
}
```

## Implementation Order

1. **Content-Type Validation** - Add gRPC-Web recognition (non-breaking)
2. **Trailer Frame Encoding** - Implement flag 0x80 for streaming responses
3. **Error Response Formatting** - Convert Connect errors to gRPC-Web format
4. **Enhanced Routing** - Improve ContentTypeSwitch with explicit variants
5. **Integration Tests** - Comprehensive protocol compatibility tests
6. **Documentation** - Update README with gRPC-Web section

## Key Insight

The fundamental difference between Connect and gRPC-Web is **metadata transmission**:

| Protocol | Trailing Metadata |
|----------|-------------------|
| Connect | JSON EndStream frame (flag 0x02) |
| gRPC | HTTP/2 trailers |
| gRPC-Web | Message frame with flag 0x80, HTTP headers as text |

This means gRPC-Web requires special handling that cannot be delegated entirely to tonic-web when we need to maintain consistent error handling and metadata propagation with Connect.
