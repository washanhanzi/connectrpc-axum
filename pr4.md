# PR 4: Unary Compression Support

## Overview

Add HTTP-native compression for unary RPCs using standard `Content-Encoding` / `Accept-Encoding` headers.

**Supported encodings:** `gzip`, `identity`

## Architecture

```
Request Flow:
┌─────────────────────────────────────────────────────────────────────┐
│ ConnectService (layer)                                              │
│  1. Read Content-Encoding header                                    │
│  2. Validate encoding (reject unsupported → Code::Unimplemented)    │
│  3. Read Accept-Encoding, negotiate response encoding               │
│  4. Store Compression { response } in extensions                    │
│  5. Read body, decompress, check size limits                        │
│  6. Replace body with decompressed bytes                            │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│ ConnectRequest extractor (request.rs)                               │
│  1. Read body bytes (already decompressed)                          │
│  2. Deserialize                                                     │
└─────────────────────────────────────────────────────────────────────┘

Response Flow:
┌─────────────────────────────────────────────────────────────────────┐
│ ConnectResponse (response.rs)                                       │
│  1. Serialize to bytes                                              │
│  2. If bytes.len() >= min_bytes && response != Identity             │
│     → Compress using Compression::response encoding                 │
│  3. Set Content-Encoding header if compressed                       │
└─────────────────────────────────────────────────────────────────────┘
```

## Types

```rust
// src/compression.rs

/// Supported compression encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionEncoding {
    #[default]
    Identity,
    Gzip,
}

/// Compression context for response.
/// Stored in request extensions by ConnectService.
#[derive(Debug, Clone, Copy, Default)]
pub struct Compression {
    /// Negotiated from Accept-Encoding header
    pub response: CompressionEncoding,
}

/// Server compression configuration.
#[derive(Debug, Clone, Copy)]
pub struct CompressionConfig {
    /// Minimum bytes before compression is applied (default: 1024)
    /// Set to usize::MAX to disable compression
    pub min_bytes: usize,
}
```

## Implementation Steps

### 1. Add dependency

```toml
# Cargo.toml
flate2 = "1.0"
```

### 2. Create compression module

**File:** `src/compression.rs`

- `CompressionEncoding` enum with `from_header()` and `as_str()`
- `Compression` struct (response encoding only, since request is handled in layer)
- `CompressionConfig` with `min_bytes` field
- `compress()` and `decompress()` functions using flate2
- `negotiate_response_encoding(accept: &str) -> CompressionEncoding`

### 3. Update ConnectService to handle decompression

**File:** `src/layer.rs`

For unary POST requests:

```rust
// In ConnectService::call, after protocol detection:

if method == Method::POST && protocol.is_unary() {
    // 1. Validate Content-Encoding
    let content_encoding = req.headers()
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());

    let request_encoding = match CompressionEncoding::from_header(content_encoding) {
        Some(enc) => enc,
        None => {
            return err_response(Code::Unimplemented,
                format!("unknown compression: supported encodings are gzip, identity"));
        }
    };

    // 2. Negotiate response encoding from Accept-Encoding
    let accept = req.headers().get(ACCEPT_ENCODING)...;
    let response_encoding = negotiate_response_encoding(accept);
    req.extensions_mut().insert(Compression { response: response_encoding });

    // 3. Read and decompress body
    if request_encoding != CompressionEncoding::Identity {
        let (parts, body) = req.into_parts();
        let bytes = to_bytes(body).await?;
        let decompressed = decompress(bytes, request_encoding)
            .map_err(|e| ConnectError::new(Code::InvalidArgument, e.to_string()))?;

        // 4. Check decompressed size against limits
        let limits = parts.extensions.get::<Limits>();
        if let Some(limits) = limits {
            limits.check_size(decompressed.len())?;
        }

        // 5. Replace body
        req = Request::from_parts(parts, Body::from(decompressed));
    }
}
```

### 4. Update ConnectLayer configuration

**File:** `src/layer.rs`

- Add `compression: CompressionConfig` field to `ConnectLayer`
- Add `.compression(config)` builder method

### 5. Compress in response

**File:** `src/message/response.rs`

```rust
let body = serialize(message)?;
let compression = /* from extensions */;
let (body, encoding) = if compression.response != Identity
    && body.len() >= config.min_bytes {
    (compress(body, compression.response)?, Some(compression.response))
} else {
    (body, None)
};
// Set Content-Encoding header if encoding.is_some()
```

### 6. Pass compression to response

**File:** `src/handler.rs`

Extract `Compression` from request extensions and pass to response serialization.

## Error Handling

| Scenario | Response |
|----------|----------|
| Unsupported Content-Encoding (e.g., `br`) | `Code::Unimplemented` with message listing supported encodings |
| Decompression failure (corrupt data) | `Code::InvalidArgument` |
| Compression failure | Fall back to uncompressed (don't fail request) |

## Edge Cases

- Missing `Content-Encoding` → treat as `identity`
- Missing `Accept-Encoding` → respond with `identity`
- GET requests → check `compression` query param (not Content-Encoding)
- Streaming requests → ignore these headers (use `Connect-Content-Encoding` instead)
- Size threshold → only compress if `body.len() >= min_bytes` (default 1024)

## Test Cases

1. **Gzip request decompression** — send gzip body, verify handler receives decompressed
2. **Gzip response compression** — send `Accept-Encoding: gzip`, verify response is compressed
3. **Small response not compressed** — body < min_bytes stays uncompressed
4. **Unsupported encoding rejected** — `Content-Encoding: br` → Unimplemented error
5. **Asymmetric compression** — uncompressed request + `Accept-Encoding: gzip` → compressed response
6. **Corrupt gzip data** — returns InvalidArgument error
