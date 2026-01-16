# Compression Architecture Plan

## Background

### Go's Approach (connect-go)
- Uses streaming codec interfaces (`io.Reader`/`io.Writer`)
- But applies them to **fully-buffered data** (per-message)
- Explicitly prevents double compression:
  - Streaming clients send `Accept-Encoding: identity` to disable HTTP body compression
  - Uses `Connect-Content-Encoding` for per-envelope compression

### Why We Choose a Simpler Design
Protocol constraints force buffering anyway:
1. **Envelope format**: `[flags:1][length:4][payload]` — must read all `length` bytes before decompression
2. **Serialization**: Protobuf/JSON require complete message for deserialization

Result: `Bytes → Bytes` API is honest about what happens and simpler to implement.

### Key Insight: Separation of Concerns
- **Unary**: Uses standard `Content-Encoding`/`Accept-Encoding` → Tower can handle this
- **Streaming**: Uses `Connect-Content-Encoding`/`Connect-Accept-Encoding` → We handle per-envelope

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│           BridgeLayer (new, thin)                 │
│  - For streaming: validate no Content-Encoding               │
│  - For streaming: override Accept-Encoding → identity        │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         Tower CompressionLayer (user-provided)         │  │
│  │  - Decompress request body (unary)                     │  │
│  │  - Compress response body (unary)                      │  │
│  │  - Supports: gzip, deflate, br, zstd                   │  │
│  │                                                        │  │
│  │  ┌──────────────────────────────────────────────────┐  │  │
│  │  │              ConnectLayer (existing)             │  │  │
│  │  │  - Protocol detection                            │  │  │
│  │  │  - Parse Connect-Content-Encoding (streaming)    │  │  │
│  │  │  - Envelope compression context                  │  │  │
│  │  │  - Timeout handling                              │  │  │
│  │  │                                                  │  │  │
│  │  │  ┌────────────────────────────────────────────┐  │  │  │
│  │  │  │              Handler                       │  │  │  │
│  │  │  │  (pipeline uses context for envelope       │  │  │  │
│  │  │  │   compression/decompression)               │  │  │  │
│  │  │  └────────────────────────────────────────────┘  │  │  │
│  │  └──────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

## Layer Responsibilities

| Layer | Request Phase | Response Phase |
|-------|---------------|----------------|
| **BridgeLayer** | Validate headers, override `Accept-Encoding` for streaming | Pass through |
| **Tower CompressionLayer** | Decompress body via `Content-Encoding` (unary) | Compress body via `Accept-Encoding` (unary) |
| **ConnectLayer** | Protocol detection, parse `Connect-Content-Encoding`, build context | Pass through |
| **Handler/Pipeline** | Decompress envelopes (streaming) | Compress envelopes + set `Connect-Content-Encoding` (streaming) |

## Request/Response Flow

### Unary RPC
```
Request:  Client [Content-Encoding: gzip]
          → BridgeLayer (pass through)
          → Tower (decompress body)
          → ConnectLayer (build context)
          → Handler (raw bytes)

Response: Handler (raw bytes)
          → ConnectLayer (pass through)
          → Tower (compress body)
          → BridgeLayer (pass through)
          → Client [Content-Encoding: gzip]
```

### Streaming RPC
```
Request:  Client [Connect-Content-Encoding: gzip, Accept-Encoding: identity]
          → BridgeLayer (validate no Content-Encoding, enforce identity)
          → Tower (does nothing - no Content-Encoding)
          → ConnectLayer (parse Connect-Content-Encoding)
          → Handler/Pipeline (decompress each envelope)

Response: Handler/Pipeline (compress each envelope)
          → ConnectLayer (pass through)
          → Tower (does nothing - Accept-Encoding: identity)
          → BridgeLayer (pass through)
          → Client [Connect-Content-Encoding: gzip]
```

## Implementation

### 1. New: BridgeLayer (`src/layer/bridge.rs`)

```rust
/// Bridges Tower compression with Connect protocol.
///
/// For streaming requests:
/// - Rejects if Content-Encoding is set (prevents double compression)
/// - Overrides Accept-Encoding to identity (prevents Tower compressing response)
///
/// Algorithm-agnostic: works with any compression layer inside.
pub struct BridgeLayer;

impl<S, B> Service<Request<B>> for CompressionBridgeService<S> {
    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        let is_streaming = is_connect_streaming(&req);

        if is_streaming {
            // Reject if Content-Encoding is set
            if let Some(ce) = req.headers().get(CONTENT_ENCODING) {
                if ce != "identity" {
                    return error_response(Code::InvalidArgument,
                        "streaming requests must not use Content-Encoding");
                }
            }

            // Force identity for Tower
            req.headers_mut().insert(ACCEPT_ENCODING, "identity");
        }

        self.inner.call(req)
    }
}

fn is_connect_streaming(req: &Request<B>) -> bool {
    req.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("application/connect+"))
        .unwrap_or(false)
}
```

### 2. Simplify: compression.rs (`src/context/message_compression.rs`)

Remove `StreamingCodec`, keep simple `Codec` trait for envelope compression:

```rust
/// Codec for per-envelope compression (streaming only).
/// HTTP body compression is handled by Tower.
pub trait Codec: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn compress(&self, data: Bytes) -> io::Result<Bytes>;
    fn decompress(&self, data: Bytes) -> io::Result<Bytes>;
}

/// Built-in gzip codec using flate2.
pub struct GzipCodec { pub level: u32 }

impl Codec for GzipCodec { ... }
```

### 3. Update: ConnectLayer (`src/layer/connect.rs`)

Add envelope compression configuration:

```rust
impl ConnectLayer {
    /// Set supported envelope compression algorithms for streaming RPCs.
    ///
    /// These are used for per-message compression in streaming calls.
    /// HTTP body compression (for unary) is handled by Tower.
    pub fn envelope_compression(mut self, algorithms: &[&str]) -> Self {
        self.config.envelope_compression = algorithms.into();
        self
    }
}
```

### 4. Update: Pipeline (envelope handling)

When reading streaming messages:
- Check envelope flags for compression
- Decompress using codec from context

When writing streaming messages:
- Compress if size >= min_bytes threshold
- Set envelope compression flag
- Set `Connect-Content-Encoding` response header

## User Configuration

### Default (HTTP gzip compression always enabled)
```rust
use connectrpc_axum::MakeServiceBuilder;

// HTTP compression is always enabled (gzip)
let app = MakeServiceBuilder::new()
    .add_router(my_router)
    .build();
```

### Manual layer stack (for advanced use)
```rust
use tower_http::compression::CompressionLayer;
use tower_http::decompression::DecompressionLayer;
use connectrpc_axum::{ConnectLayer, BridgeLayer};

let app = Router::new()
    .route("/service/Method", post(handler))
    .layer(ConnectLayer::new())
    .layer(CompressionLayer::new())
    .layer(DecompressionLayer::new())
    .layer(BridgeLayer::new());
```

## Files Changed

| File | Change | Status |
|------|--------|--------|
| `src/layer.rs` | Module file for layer submodules | ✅ Done |
| `src/layer/bridge.rs` | **New**: BridgeLayer | ✅ Done |
| `src/layer/connect.rs` | **Moved**: ConnectLayer from old layer.rs | ✅ Done |
| `src/context/message_compression.rs` | **New**: Simplified Codec trait, BoxedCodec, GzipCodec | ✅ Done |
| `src/context/compression.rs` | **Removed**: Old file with unused StreamingCodec | ✅ Done |
| `src/context.rs` | **Update**: Re-export from message_compression | ✅ Done |
| `src/lib.rs` | **Update**: Re-export BridgeLayer, Codec, BoxedCodec | ✅ Done |
| `src/service_builder.rs` | **Update**: Add tower-http compression layer, enabled by default | ✅ Done |
| `Cargo.toml` (package) | **Update**: Add tower-http dependency | ✅ Done |

## Testing

1. **Unary + gzip**: Verify Tower handles body compression
2. **Streaming + gzip**: Verify envelope compression works
3. **Double compression rejection**: Verify streaming with Content-Encoding is rejected
4. **No compression**: Verify identity works
5. **Mixed**: Unary compressed, streaming envelope-compressed in same service
