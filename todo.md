# Fix Plan: connectrpc-axum Protocol & Safety Issues

## Reference: How connect-go handles this

connect-go (`/tmp/connect-go/protocol_connect.go`) provides the canonical implementation:

1. **No thread-locals** - Protocol/codec info is computed once per request and stored on a per-request "connection" object (`connectUnaryHandlerConn` or `connectStreamingHandlerConn`) that carries state for that call's lifetime.

2. **Content-Type set upfront** - In `NewConn()`, before any body bytes are written:
   ```go
   header[headerContentType] = []string{contentType}  // line 222
   ```

3. **Unary vs Streaming encoding**:
   - Unary uses `application/<codec>` and writes **raw bytes, no envelope** (`connectUnaryMarshaler.write()` lines 964-975)
   - Streaming uses `application/connect+<codec>` with **envelope frames** (`connectStreamingMarshaler` embeds `envelopeWriter`)

4. **Frame validation** - `envelopeReader.Read()` (lines 317-375):
   - Uses `io.ReadFull` to require exactly 5 bytes for prefix
   - Returns "protocol error: incomplete envelope" on short reads
   - Returns "protocol error: promised %d bytes in enveloped message, got %d bytes" on length mismatch

---

## Critical Priority

### 1. Thread-local format storage causes race conditions
**Files:** `connectrpc-axum/src/request.rs:27-38`, `connectrpc-axum/src/response.rs:34-36,104-105`

**Problem:** `REQUEST_FORMAT` is stored in thread-local storage. This is fundamentally broken for async Rust:
- Concurrent tasks on the same thread can overwrite each other's format
- Async tasks can hop between threads (tokio work-stealing), making the format nondeterministic
- Under load, responses can be encoded in the wrong format

**Fix:** Follow connect-go's approach - create a per-request connection/context object.

**Options:**

1. **Option A (Recommended): Per-request connection struct via middleware**

   Create a middleware layer that:
   1. Detects protocol/codec from incoming request (headers or query params)
   2. Creates a `ConnectContext` and stores it in request extensions
   3. Sets `Content-Type` response header upfront (like connect-go line 222)
   4. Handles response encoding based on stored protocol

   This mirrors connect-go's `NewConn()` pattern. The middleware owns the encoding decision since `IntoResponse` can't access request extensions.

2. **Option B: Explicit context threading**

   Modify `ConnectRequest<T>` to carry protocol info, require handlers to pass it to `ConnectResponse`:
   ```rust
   pub struct ConnectRequest<T> {
       pub message: T,
       pub protocol: RequestProtocol,
   }

   impl ConnectResponse<T> {
       pub fn new(message: T, protocol: RequestProtocol) -> Self { ... }
   }
   ```

**Implementation sketch (Option A):**
```rust
/// Per-request protocol context (mirrors connect-go's handlerConn)
#[derive(Debug, Clone, Copy)]
pub struct ConnectContext {
    pub protocol: RequestProtocol,
    pub codec: Codec,
}

pub struct ConnectLayer;

impl<S> Layer<S> for ConnectLayer {
    type Service = ConnectService<S>;
    // ...
}

impl<S, B> Service<Request<B>> for ConnectService<S> {
    async fn call(&mut self, mut req: Request<B>) -> Response {
        // 1. Detect protocol from Content-Type header or query params
        let ctx = ConnectContext::from_request(&req);

        // 2. Store in extensions for extractors
        req.extensions_mut().insert(ctx);

        // 3. Call inner service
        let mut response = self.inner.call(req).await;

        // 4. Set Content-Type header upfront (like connect-go)
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            ctx.response_content_type(),
        );

        // 5. Apply encoding/framing if needed
        ctx.encode_response(response)
    }
}
```

---

## High Priority

### 2. Proto responses incorrectly use gRPC framing for Connect clients
**Files:** `connectrpc-axum/src/response.rs:38-45,161-165`

**Problem:** When `use_proto = true`, responses always use:
- `application/grpc+proto` content-type
- gRPC 5-byte frame envelope

But Connect protocol content-types are:
- **Unary:** `application/proto` or `application/json` (no `connect+` prefix)
- **Streaming:** `application/connect+proto` or `application/connect+json`

So Connect unary clients posting `application/proto` expect:
- `application/proto` content-type in response
- **No frame envelope** (raw protobuf bytes)

Connect streaming clients posting `application/connect+proto` expect:
- `application/connect+proto` content-type
- Frame envelope with Connect framing (flags 0x00 for data, 0x02 for end-stream)

**Reference:** connect-go's response encoding:
- Unary success: raw bytes with `application/<codec>` (lines 964-975)
- Unary error: **always JSON** with `application/json` regardless of request codec (line 742)
- Streaming: envelope frames with `application/connect+<codec>`

**Fix:**
1. Track not just proto vs json, but also the protocol variant (gRPC vs Connect) and stream type
2. For Connect unary `application/proto`: return raw protobuf bytes, no frame
3. For Connect unary errors: always return JSON (per Connect spec)
4. For Connect streaming `application/connect+proto`: return framed response with `application/connect+proto`
5. For gRPC: return framed response with `application/grpc+proto`

**Implementation:**
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RequestProtocol {
    ConnectUnaryJson,        // application/json → respond application/json, no frame
    ConnectUnaryProto,       // application/proto → respond application/proto, no frame
                             //                     errors: always application/json
    ConnectStreamJson,       // application/connect+json → framed connect+json
    ConnectStreamProto,      // application/connect+proto → framed connect+proto
    GrpcProto,               // application/grpc* → framed grpc+proto
}

impl RequestProtocol {
    /// Response content-type for successful responses
    pub fn response_content_type(&self) -> &'static str {
        match self {
            Self::ConnectUnaryJson => "application/json",
            Self::ConnectUnaryProto => "application/proto",
            Self::ConnectStreamJson => "application/connect+json",
            Self::ConnectStreamProto => "application/connect+proto",
            Self::GrpcProto => "application/grpc+proto",
        }
    }

    /// For Connect unary, errors are always JSON (per spec)
    pub fn error_content_type(&self) -> &'static str {
        match self {
            Self::ConnectUnaryJson | Self::ConnectUnaryProto => "application/json",
            Self::ConnectStreamJson => "application/connect+json",
            Self::ConnectStreamProto => "application/connect+proto",
            Self::GrpcProto => "application/grpc+proto",
        }
    }

    pub fn needs_envelope(&self) -> bool {
        matches!(self,
            Self::ConnectStreamJson |
            Self::ConnectStreamProto |
            Self::GrpcProto
        )
    }
}
```

---

### 3. Framed request parsing silently accepts malformed frames
**Files:** `connectrpc-axum/src/request.rs:100-130`

**Problem:**
- Line 100: `if needs_frame_unwrap && bytes.len() >= 5` silently accepts < 5 byte frames (falls through to decode, likely failing with confusing error)
- Line 129-130: Empty else branch when `bytes.len() < 5` - should be an error
- Lines 105-114: Validates flags=1 (compression) but doesn't decompress - should error or actually decompress

**Reference:** connect-go's `envelopeReader.Read()` (envelope.go:317-375):
- Uses `io.ReadFull` requiring exactly 5 bytes - returns "protocol error: incomplete envelope" on short read
- Validates payload length matches header - returns "protocol error: promised %d bytes, got %d"
- Handles compression flag properly at unmarshal time

**Fix:**
```rust
if needs_frame_unwrap {
    // Require exactly 5-byte header (like connect-go's io.ReadFull)
    if bytes.len() < 5 {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "protocol error: incomplete envelope",
        ));
    }

    let flags = bytes[0];
    let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;

    // Validate payload length matches header
    if bytes.len() - 5 != length {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "protocol error: promised {} bytes in enveloped message, got {} bytes",
                length,
                bytes.len() - 5
            ),
        ));
    }

    // Reject compression flag - we don't support decompression yet
    // (connect-go returns "sent compressed message without compression support")
    if flags & 0x01 != 0 {
        return Err(ConnectError::new(
            Code::Internal,
            "protocol error: sent compressed message without compression support",
        ));
    }

    // For streaming, flags 0x02 indicates end-of-stream (connect-go: connectFlagEnvelopeEndStream)
    // This is valid for Connect streaming but should be handled specially
    // For gRPC, only 0x00 and 0x01 are valid
    if is_grpc && flags > 1 {
        return Err(ConnectError::new(
            Code::Internal,
            format!("protocol error: invalid envelope flags {}", flags),
        ));
    }

    bytes = bytes.slice(5..);
}
```

---

## Medium Priority

### 4. GET requests don't set response format
**Files:** `connectrpc-axum/src/request.rs:163-201`

**Problem:** `from_get_request` parses `encoding` parameter but never calls `set_request_format()`. Response will always use JSON (the default) even when `encoding=proto`.

**Fix:**
```rust
async fn from_get_request<S, T>(req: Request, _state: &S) -> Result<ConnectRequest<T>, ConnectError> {
    // ... existing parsing ...

    // Set format based on encoding parameter
    let format = if params.encoding == "proto" {
        ContentFormat::Proto  // or RequestProtocol::ConnectUnaryProto after refactor
    } else {
        ContentFormat::Json
    };
    set_request_format(format);  // Or use extensions after refactor

    // ... rest of function
}
```

**Note:** This fix should be combined with the thread-local refactor (Issue #1). After that refactor, this becomes storing the format in request extensions.

---


### 5. `unwrap()` calls can panic on serialization failures
**Files:**
- `connectrpc-axum/src/response.rs:42,48,59`
- `connectrpc-axum/src/response.rs:119,121,133`
- `connectrpc-axum/src/stream_response.rs:58,67,78`

**Problem:** Multiple `.unwrap()` calls on serialization operations:
```rust
self.0.encode(&mut buf).unwrap();       // protobuf encode can fail
serde_json::to_vec(&self.0).unwrap();   // json encode can fail
Response::builder()...body(...).unwrap(); // builder can fail
```

**Fix:** Return proper errors. For `IntoResponse` trait, this is tricky since the trait returns `Response` not `Result<Response, _>`. Options:

1. **Return error response on failure:**
```rust
fn into_response(self) -> Response {
    match self.try_encode() {
        Ok(response) => response,
        Err(e) => ConnectError::new(Code::Internal, e.to_string()).into_response(),
    }
}
```

2. **Use `expect()` with clear message for truly impossible failures:**
```rust
Response::builder()
    .status(StatusCode::OK)
    .header(...)
    .body(Body::from(body))
    .expect("building response with valid status and headers cannot fail")
```

**Recommended:** Mix of both - use `expect()` for Response builder (which truly cannot fail with valid inputs), but handle encode errors gracefully by returning error responses.

---

## Implementation Order

1. **Issue #1 (Critical)** - Thread-local removal - This is foundational; other fixes depend on proper format propagation
2. **Issue #2 (High)** - Protocol detection & response encoding - Depends on #1's format propagation design
3. **Issue #3 (High)** - Frame validation - Independent, can be done in parallel with #1/#2
4. **Issue #4 (Medium)** - GET format setting - Trivial after #1 is done
6. **Issue #5 (Low)** - Unwrap removal - Independent, can be done anytime

## Testing Strategy

Reference: connect-go tests in `connect_ext_test.go` for frame validation edge cases.

- Add integration tests with concurrent requests mixing proto/json (verifies thread-local fix)
- Test Connect unary clients with `application/proto` - verify response is raw bytes, no frame, with `application/proto` content-type
- Test Connect streaming with `application/connect+proto` - verify envelope framing and end-stream message
- Test malformed frame handling:
  - Undersized frames (< 5 bytes) → "protocol error: incomplete envelope"
  - Wrong payload length → "protocol error: promised N bytes, got M"
  - Compression flag set → "sent compressed message without compression support"
  - Invalid flags for gRPC → "protocol error: invalid envelope flags"
- Test GET requests with `encoding=proto` - verify proto response encoding
- Verify no panics under serialization edge cases (empty messages, huge messages)
- Test gRPC vs Connect protocol detection and correct response encoding for each
