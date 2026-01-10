# Compression

## Basic Usage

Configure response compression using `MakeServiceBuilder`:

```rust
use connectrpc_axum::{MakeServiceBuilder, CompressionConfig};

MakeServiceBuilder::new()
    .add_router(router)
    .compression(CompressionConfig::new(512))  // Compress responses >= 512 bytes
    .build()
```

## Supported Encodings

| Encoding | Description |
|----------|-------------|
| `gzip` | Gzip compression via flate2 |
| `identity` | No compression (passthrough) |

## Accept-Encoding Negotiation

The server negotiates response compression using standard HTTP headers:

- **Unary RPCs**: `Accept-Encoding` header
- **Streaming RPCs**: `Connect-Accept-Encoding` header

### How Negotiation Works

Following connect-go's approach:

1. **First supported encoding wins** - client preference order is respected
2. **`q=0` means disabled** - encodings with `q=0` are skipped per RFC 7231
3. **Other q-values are ignored** - no preference weighting beyond order

| Accept-Encoding | Result |
|-----------------|--------|
| `gzip` | gzip |
| `gzip, deflate, br` | gzip (first supported) |
| `br, zstd, gzip` | gzip (first supported) |
| `identity, gzip` | identity (first supported) |
| `gzip;q=0` | identity (gzip disabled) |
| `gzip;q=0, identity` | identity (gzip disabled) |
| `gzip;q=0.5` | gzip (non-zero q accepted) |
| `deflate, br` | identity (none supported) |

::: tip
Unlike full HTTP content negotiation, Connect protocol doesn't weight by q-value—it uses client order. This matches connect-go's behavior.
:::

## Configuration Options

### Minimum Bytes Threshold

Only compress responses larger than a threshold (default: 1024 bytes):

```rust
use connectrpc_axum::CompressionConfig;

// Compress responses >= 512 bytes
let config = CompressionConfig::new(512);

// Default: 1024 bytes
let config = CompressionConfig::default();
```

Small messages often don't benefit from compression due to overhead.

### Disabling Compression

Disable response compression entirely:

```rust
use connectrpc_axum::CompressionConfig;

MakeServiceBuilder::new()
    .add_router(router)
    .compression(CompressionConfig::disabled())
    .build()
```

## Request Decompression

The server automatically decompresses incoming requests based on:

- **Unary RPCs**: `Content-Encoding` header
- **Streaming RPCs**: `Connect-Content-Encoding` header

Unsupported encodings return `Unimplemented` error:

```
unsupported compression "br": supported encodings are gzip, identity
```

## Streaming Compression

For streaming RPCs, compression is applied per-message using the envelope format:

- Each message frame has a compression flag (byte 0, bit 0x01)
- Compressed frames are automatically decompressed on read
- Response frames are compressed based on negotiated encoding and size threshold

## Protocol Headers

| RPC Type | Request Compression | Response Negotiation |
|----------|---------------------|----------------------|
| Unary | `Content-Encoding` | `Accept-Encoding` |
| Streaming | `Connect-Content-Encoding` | `Connect-Accept-Encoding` |

## Implementation Notes

### Conformance with connect-go

This implementation matches connect-go's compression behavior:

- First-match-wins negotiation (no q-value weighting)
- Respects `q=0` as "not acceptable"
- Same header names for unary vs streaming
- Same error messages for unsupported encodings

### Custom Codecs

For custom compression algorithms (zstd, brotli), implement the `Codec` trait:

```rust
use connectrpc_axum::compression::Codec;
use bytes::Bytes;
use std::io;

struct ZstdCodec { level: i32 }

impl Codec for ZstdCodec {
    fn name(&self) -> &'static str { "zstd" }

    fn compress(&self, data: Bytes) -> io::Result<Bytes> {
        // ... zstd compression
    }

    fn decompress(&self, data: Bytes) -> io::Result<Bytes> {
        // ... zstd decompression
    }
}
```

::: warning
Custom codecs require additional wiring to integrate with the negotiation logic. This API is subject to change.
:::
