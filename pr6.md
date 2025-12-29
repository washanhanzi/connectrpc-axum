# PR 6 — GET Unary Request Strictness

## Problem Summary

The current GET request handling in connectrpc-axum lacks the strict validation that connect-go enforces. GET requests are user-visible (URLs appear in browser history, logs, etc.) and cache-sensitive, requiring stricter validation than POST requests.

### Current Issues

1. **Missing `connect=v1` validation in layer** - Checked only late in `from_get_request()`, not at the middleware level
2. **No encoding enforcement** - Protocol detection defaults to JSON silently if `encoding` parameter is missing
3. **Invalid encoding values accepted** - No rejection of unsupported encoding values like `xml` or arbitrary strings
4. **Compression parameter unused** - Query parameter is parsed but marked `#[allow(dead_code)]`
5. **Wrong HTTP status codes** - Returns HTTP 200 with Connect error codes instead of HTTP 415 for invalid requests

## connect-go Reference Behavior

From `connect-go/protocol_connect.go`:

```go
// Query parameter constants (lines 37-60)
const (
    connectUnaryEncodingQueryParameter    = "encoding"
    connectUnaryMessageQueryParameter     = "message"
    connectUnaryBase64QueryParameter      = "base64"
    connectUnaryCompressionQueryParameter = "compression"
    connectUnaryConnectQueryParameter     = "connect"
    connectUnaryConnectQueryValue         = "v1"
)
```

**Validation requirements (lines 184-212):**
1. `encoding` parameter is **required**
2. `message` parameter is **required**
3. Encoding must match a supported codec (json, proto)
4. `connect=v1` must be present and correct
5. `compression` is optional but validated against supported algorithms

## Proposed Changes

### 1. Add GET Query Validation to Layer Middleware

**File:** `connectrpc-axum/src/layer.rs`

Add GET validation block similar to existing POST validation:

```rust
// After detecting protocol, before calling inner service
if *req.method() == Method::GET {
    if let Some(error) = validate_get_query_params(&req) {
        return Box::pin(async move {
            Ok(error.into_response(protocol))
        });
    }
}
```

### 2. Create GET Validation Function

**File:** `connectrpc-axum/src/layer/content_type.rs` (or new `layer/get_request.rs`)

```rust
pub fn validate_get_query_params(req: &Request) -> Option<(Code, &'static str, u16)> {
    let query = req.uri().query().unwrap_or("");
    let params: HashMap<&str, &str> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .collect();

    // Require connect=v1
    match params.get("connect") {
        None => return Some((Code::InvalidArgument,
            "missing required query parameter: set connect to \"v1\"", 400)),
        Some(&v) if v != "v1" => return Some((Code::InvalidArgument,
            "connect must be \"v1\"", 400)),
        _ => {}
    }

    // Require encoding parameter
    let encoding = match params.get("encoding") {
        None => return Some((Code::InvalidArgument,
            "missing encoding parameter", 400)),
        Some(&v) => v,
    };

    // Validate encoding is supported
    if encoding != "json" && encoding != "proto" {
        return Some((Code::InvalidArgument,
            "invalid message encoding", 415)); // 415 Unsupported Media Type
    }

    // Validate compression if present
    if let Some(&compression) = params.get("compression") {
        if compression != "gzip" && compression != "identity" && !compression.is_empty() {
            return Some((Code::InvalidArgument,
                "unsupported compression", 415));
        }
    }

    None
}
```

### 3. Implement Compression Decompression for GET

**File:** `connectrpc-axum/src/message/request.rs`

Update `from_get_request()` to use compression parameter:

```rust
async fn from_get_request<S, T>(req: Request, _state: &S) -> Result<ConnectRequest<T>, ConnectError>
{
    let query = req.uri().query().unwrap_or("");
    let params: GetRequestQuery = serde_qs::from_str(query)
        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;

    // Decode message bytes
    let message_bytes = if params.base64.as_deref() == Some("1") {
        URL_SAFE.decode(&params.message)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        params.message.into_bytes()
    };

    // Apply decompression if specified
    let message_bytes = if let Some(ref compression) = params.compression {
        if compression == "gzip" {
            decompress(CompressionEncoding::Gzip, &message_bytes)
                .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
        } else {
            message_bytes
        }
    } else {
        message_bytes
    };

    // Continue with deserialization...
}
```

### 4. Update Error Response Status Codes

**File:** `connectrpc-axum/src/error.rs`

Ensure validation errors return appropriate HTTP status codes:

| Error | HTTP Status |
|-------|-------------|
| Missing `connect` parameter | 400 Bad Request |
| Invalid `connect` value | 400 Bad Request |
| Missing `encoding` parameter | 400 Bad Request |
| Unsupported encoding | 415 Unsupported Media Type |
| Unsupported compression | 415 Unsupported Media Type |

## Files to Modify

| File | Lines | Changes |
|------|-------|---------|
| `layer.rs` | ~189-251 | Add GET validation block |
| `layer/content_type.rs` | New function | Add `validate_get_query_params()` |
| `message/request.rs` | 148-197 | Implement compression decompression |
| `error.rs` | As needed | Ensure 415 status code support |

## Testing Requirements

```rust
#[test]
fn test_get_missing_connect() {
    // GET /method?encoding=json&message={}
    // Expected: 400 Bad Request, "missing required query parameter"
}

#[test]
fn test_get_invalid_connect() {
    // GET /method?connect=v2&encoding=json&message={}
    // Expected: 400 Bad Request, "connect must be v1"
}

#[test]
fn test_get_missing_encoding() {
    // GET /method?connect=v1&message={}
    // Expected: 400 Bad Request, "missing encoding parameter"
}

#[test]
fn test_get_invalid_encoding() {
    // GET /method?connect=v1&encoding=xml&message={}
    // Expected: 415 Unsupported Media Type
}

#[test]
fn test_get_unsupported_compression() {
    // GET /method?connect=v1&encoding=json&compression=br&message={}
    // Expected: 415 Unsupported Media Type
}

#[test]
fn test_get_valid_with_compression() {
    // GET /method?connect=v1&encoding=proto&compression=gzip&base64=1&message=<gzipped-base64>
    // Expected: 200 OK with decoded response
}
```

## Implementation Order

1. Add query parameter validation function
2. Integrate validation into layer middleware for GET requests
3. Implement compression decompression in `from_get_request()`
4. Update error handling to return correct HTTP status codes
5. Add comprehensive integration tests
