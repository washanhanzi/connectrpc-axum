# Compression

## Default Compression

When using `build_connect()` on a service builder, gzip compression is enabled by default — provided a gzip compression feature (e.g. `compression-gzip`) is enabled. Compression features are off by default, so without one `build_connect()` applies no HTTP-body compression:

```rust
let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .build_connect();  // Includes gzip compression when a gzip feature is enabled
```

This uses `MakeServiceBuilder::new()` internally, which provides:
- Gzip compression and decompression by default (when a gzip compression feature is enabled)
- Standard ConnectLayer configuration

## Custom Configuration

For custom compression settings, use `MakeServiceBuilder` directly:

```rust
use connectrpc_axum::{MakeServiceBuilder, CompressionConfig, CompressionLevel};

MakeServiceBuilder::new()
    .add_router(router)
    .compression(
        CompressionConfig::new(512)           // Compress responses >= 512 bytes
            .level(CompressionLevel::Default) // Use default compression level
    )
    .build()
```

## Compression Level

Control the compression quality/speed tradeoff:

```rust
use connectrpc_axum::{CompressionConfig, CompressionLevel};

// Fastest compression (larger output, less CPU)
let config = CompressionConfig::default().level(CompressionLevel::Fastest);

// Best compression (smaller output, more CPU)
let config = CompressionConfig::default().level(CompressionLevel::Best);

// Algorithm default (recommended for most cases)
let config = CompressionConfig::default().level(CompressionLevel::Default);

// Precise level (algorithm-specific, clamped to max)
let config = CompressionConfig::default().level(CompressionLevel::Precise(6));
```

| Level | Description |
|-------|-------------|
| `Fastest` | Prioritize speed over compression ratio |
| `Best` | Prioritize compression ratio over speed |
| `Default` | Algorithm's default balance |
| `Precise(n)` | Exact level (clamped to algorithm's max) |

::: tip
The compression level applies uniformly to all enabled algorithms (gzip, deflate, br, zstd). Tower-http does not support per-algorithm level configuration.
:::

## Supported Encodings

| Encoding | Feature Flag |
|----------|--------------|
| `gzip` | `compression-gzip` |
| `deflate` | `compression-deflate` |
| `br` | `compression-br` |
| `zstd` | `compression-zstd` |
| `identity` | (always enabled) |

Enable additional algorithms in `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum = { version = "0.2.1", features = ["compression-br", "compression-zstd"] }

# Or enable all
connectrpc-axum = { version = "0.2.1", features = ["compression-full"] }
```

## Configuration Options

### Minimum Bytes Threshold

Only compress responses larger than a threshold:

```rust
use connectrpc_axum::CompressionConfig;

// Compress responses >= 512 bytes
let config = CompressionConfig::new(512);

// Default: 0 bytes (compress everything, matching connect-go)
let config = CompressionConfig::default();
```

## Request Decompression

The server automatically decompresses incoming requests. For streaming RPCs and GET requests, unsupported encodings return a Connect `Unimplemented` error listing enabled encodings. Unary request decompression is handled by tower-http's request decompression layer, which does not produce a Connect error.

## gRPC Compression (Tonic)

gRPC compression is configured separately using Tonic's built-in methods. See [Tonic Integration → gRPC Compression](./tonic.md#grpc-compression).

::: tip
`MakeServiceBuilder::compression()` only affects Connect RPCs. gRPC compression is handled by Tonic.
:::

## Implementation Notes

### Architecture

- **Unary RPCs**: HTTP body compression via Tower's `CompressionLayer` (`Accept-Encoding` / `Content-Encoding`)
- **Streaming RPCs**: Per-envelope compression (`Connect-Accept-Encoding` / `Connect-Content-Encoding`)

### Protocol Headers

| RPC Type | Request | Response Negotiation |
|----------|---------|----------------------|
| Unary | `Content-Encoding` | `Accept-Encoding` |
| Streaming | `Connect-Content-Encoding` | `Connect-Accept-Encoding` |

### Negotiation

Following connect-go: first supported encoding wins, `q=0` means disabled, other q-values ignored.

### Conformance with connect-go

- Default `min_bytes` is 0
- Streaming compression only when `Connect-Accept-Encoding` present
- First-match-wins negotiation (no q-value weighting)
- Respects `q=0` as "not acceptable"
