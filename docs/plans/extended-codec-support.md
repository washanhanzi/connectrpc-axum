# Extended Codec Support

This document outlines the plan for extending compression codec support in connectrpc-axum.

## Current State

The crate now has a `Codec` trait that provides a clean abstraction for compression/decompression:

```rust
pub trait Codec: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn compress(&self, data: Bytes) -> io::Result<Bytes>;
    fn decompress(&self, data: Bytes) -> io::Result<Bytes>;
}
```

Built-in implementations:
- `IdentityCodec`: Zero-copy passthrough (no compression)
- `GzipCodec`: Gzip compression via flate2

## Planned Enhancements

### 1. Additional Built-in Codecs

Add support for commonly used compression algorithms via feature flags:

```toml
[features]
zstd = ["dep:zstd"]
brotli = ["dep:brotli"]
```

#### ZstdCodec

```rust
#[cfg(feature = "zstd")]
pub struct ZstdCodec {
    pub level: i32,  // 1-22, default 3
}

impl Codec for ZstdCodec {
    fn name(&self) -> &'static str { "zstd" }
    // ...
}
```

#### BrotliCodec

```rust
#[cfg(feature = "brotli")]
pub struct BrotliCodec {
    pub quality: u32,  // 0-11, default 4
}

impl Codec for BrotliCodec {
    fn name(&self) -> &'static str { "br" }
    // ...
}
```

### 2. Codec Registry

Add a registry to `ConnectLayer` for runtime codec selection:

```rust
#[derive(Clone)]
pub struct CodecRegistry {
    codecs: HashMap<&'static str, Arc<dyn Codec>>,
}

impl CodecRegistry {
    pub fn new() -> Self;
    pub fn with_gzip(self) -> Self;

    #[cfg(feature = "zstd")]
    pub fn with_zstd(self, level: i32) -> Self;

    #[cfg(feature = "brotli")]
    pub fn with_brotli(self, quality: u32) -> Self;

    /// Register a custom codec
    pub fn register<C: Codec>(self, codec: C) -> Self;

    /// Lookup codec by name (from Content-Encoding header)
    pub fn get(&self, name: &str) -> Option<Arc<dyn Codec>>;
}
```

### 3. Layer Configuration

Update `ConnectLayer` to accept codec configuration:

```rust
// Note: ConnectLayer will change from Copy to Clone due to Arc<CodecRegistry>
#[derive(Clone)]
pub struct ConnectLayer {
    config: ServerConfig,
}

impl ConnectLayer {
    /// Configure available compression codecs.
    ///
    /// Default: identity + gzip
    pub fn codecs(mut self, registry: CodecRegistry) -> Self;
}
```

### 4. Usage Examples

```rust
// Default (identity + gzip)
let layer = ConnectLayer::new();

// Add zstd support
let layer = ConnectLayer::new()
    .codecs(
        CodecRegistry::new()
            .with_gzip()
            .with_zstd(3)
    );

// Custom codec
struct MyCodec;
impl Codec for MyCodec {
    fn name(&self) -> &'static str { "x-custom" }
    fn compress(&self, data: Bytes) -> io::Result<Bytes> { /* ... */ }
    fn decompress(&self, data: Bytes) -> io::Result<Bytes> { /* ... */ }
}

let layer = ConnectLayer::new()
    .codecs(
        CodecRegistry::new()
            .with_gzip()
            .register(MyCodec)
    );
```

## Implementation Steps

1. **Phase 1** (Done)
   - [x] Add `Codec` trait with `Bytes` signature
   - [x] Implement `IdentityCodec` and `GzipCodec`
   - [x] Refactor `compress`/`decompress` functions to use codecs

2. **Phase 2** (Future)
   - [ ] Add `CodecRegistry` struct
   - [ ] Update `ServerConfig` to hold `Arc<CodecRegistry>`
   - [ ] Change `ConnectLayer` from `Copy` to `Clone`
   - [ ] Update header parsing to use registry lookup

3. **Phase 3** (Future)
   - [ ] Add `zstd` feature and `ZstdCodec`
   - [ ] Add `brotli` feature and `BrotliCodec`
   - [ ] Update documentation and examples

## Breaking Changes

- `ConnectLayer` will change from `Copy` to `Clone` when registry is added
- `compress`/`decompress` function signatures changed from `&[u8]` to `Bytes`

## Dependencies

| Codec | Crate | Feature Flag |
|-------|-------|--------------|
| Gzip | `flate2` | (default) |
| Zstd | `zstd` | `zstd` |
| Brotli | `brotli` | `brotli` |
