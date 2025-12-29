# Pipeline Pattern: Consolidating Scattered Logic

## Problem Statement

Currently, cross-cutting concerns are scattered across multiple files:

| Concern | Config | Parsing/Negotiation | Request-side | Response-side |
|---------|--------|---------------------|--------------|---------------|
| **Compression** | `ConnectLayer` | `layer.rs` | `request.rs` | `response.rs` |
| **Timeout** | `ConnectLayer` | `layer/timeout.rs` | `layer.rs` (enforcement) | - |
| **Limits** | `ConnectLayer` | - | `request.rs` | - |
| **Protocol** | - | `layer.rs` | `request.rs` | `response.rs`, `handler.rs` |

This makes it hard to:
1. Understand how a single feature works end-to-end
2. Add new features without touching many files
3. Test features in isolation

---

## Chosen Approach: Feature Modules + Pipeline Builder

Combine self-contained feature modules with a pipeline that composes them.

### Directory Structure

```
src/
├── pipeline/
│   ├── mod.rs              # Pipeline, RequestPipeline, ResponsePipeline
│   ├── config.rs           # PipelineConfig (combines all feature configs)
│   └── context.rs          # PipelineContext (per-request negotiated state)
│
├── features/
│   ├── mod.rs              # re-exports all features
│   │
│   ├── compression/
│   │   ├── mod.rs          # Compressor, Decompressor
│   │   ├── config.rs       # CompressionConfig
│   │   ├── encoding.rs     # CompressionEncoding enum
│   │   ├── negotiate.rs    # parse headers, negotiate response encoding
│   │   ├── compress.rs     # compress() function
│   │   └── decompress.rs   # decompress() function
│   │
│   ├── timeout/
│   │   ├── mod.rs          # TimeoutEnforcer
│   │   ├── config.rs       # server_timeout: Option<Duration>
│   │   └── parse.rs        # parse Connect-Timeout-Ms, compute effective
│   │
│   ├── limits/
│   │   ├── mod.rs          # LimitChecker
│   │   └── config.rs       # MessageLimits
│   │
│   └── protocol/
│       ├── mod.rs          # Decoder, Encoder
│       ├── detect.rs       # detect from Content-Type
│       ├── decode.rs       # proto/json deserialization
│       └── encode.rs       # proto/json serialization
```

### Core Types

```rust
// ============================================================
// pipeline/config.rs - Static server configuration
// ============================================================

/// Server-wide configuration for the Connect pipeline.
/// Set once at startup, immutable per-request.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub compression: CompressionConfig,
    pub limits: MessageLimits,
    pub server_timeout: Option<Duration>,
    pub require_protocol_header: bool,
}

// ============================================================
// pipeline/context.rs - Per-request negotiated state
// ============================================================

/// Per-request context built from headers and config.
/// Created in layer, flows through request → handler → response.
/// Pipelines are reconstructed from this context when needed.
#[derive(Debug, Clone, Copy)]
pub struct PipelineContext {
    pub protocol: RequestProtocol,
    pub compression: CompressionContext,
    pub timeout: TimeoutContext,
    pub limits: MessageLimits,
}

#[derive(Debug, Clone, Copy)]
pub struct CompressionContext {
    /// Encoding of incoming request body (from Content-Encoding)
    pub request_encoding: CompressionEncoding,
    /// Negotiated encoding for response (from Accept-Encoding)
    pub response_encoding: CompressionEncoding,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeoutContext {
    /// Effective timeout = min(server, client)
    pub effective: Option<Duration>,
}

// ============================================================
// pipeline/context.rs - Context construction
// ============================================================

impl PipelineContext {
    /// Build context from request headers and server config.
    /// Called by ConnectLayer. Returns error if headers are invalid.
    pub fn from_request<B>(
        req: &Request<B>,
        config: &PipelineConfig,
    ) -> Result<Self, ConnectError> {
        // 1. Detect protocol
        let protocol = protocol::detect(&req);

        // 2. Validate & negotiate compression
        let compression = compression::negotiate(&req, &config.compression)?;

        // 3. Parse timeout
        let timeout = timeout::parse(&req, config.server_timeout);

        Ok(Self {
            protocol,
            compression,
            timeout,
            limits: config.limits,
        })
    }
}

// ============================================================
// pipeline/mod.rs - Pipeline builders (reconstructed from context)
// ============================================================

/// Processes incoming request bytes into a decoded message.
/// Reconstructed from PipelineContext in the extractor/handler.
pub struct RequestPipeline<'a> {
    ctx: &'a PipelineContext,
}

impl<'a> RequestPipeline<'a> {
    /// Reconstruct pipeline from context.
    pub fn new(ctx: &'a PipelineContext) -> Self {
        Self { ctx }
    }

    /// Process raw body bytes into decoded message.
    pub fn decode<T: Message + DeserializeOwned>(
        &self,
        body: Bytes,
    ) -> Result<T, ConnectError> {
        // 1. Decompress if needed
        let body = compression::decompress(&body, self.ctx.compression.request_encoding)?;

        // 2. Check size limits
        self.ctx.limits.check(body.len())?;

        // 3. Decode based on protocol
        protocol::decode(&body, self.ctx.protocol)
    }
}

/// Processes handler response into HTTP response bytes.
/// Reconstructed from PipelineContext in the handler wrapper.
pub struct ResponsePipeline<'a> {
    ctx: &'a PipelineContext,
    min_compress_bytes: usize,
}

impl<'a> ResponsePipeline<'a> {
    /// Reconstruct pipeline from context.
    pub fn new(ctx: &'a PipelineContext, min_compress_bytes: usize) -> Self {
        Self { ctx, min_compress_bytes }
    }

    /// Encode message and optionally compress.
    /// Returns (body_bytes, content_encoding_header).
    pub fn encode<T: Message + Serialize>(
        &self,
        message: &T,
    ) -> Result<(Vec<u8>, Option<&'static str>), ConnectError> {
        // 1. Encode based on protocol
        let body = protocol::encode(message, self.ctx.protocol)?;

        // 2. Compress if beneficial
        let encoding = self.ctx.compression.response_encoding;
        if encoding != CompressionEncoding::Identity && body.len() >= self.min_compress_bytes {
            let compressed = compression::compress(&body, encoding)?;
            Ok((compressed, Some(encoding.as_str())))
        } else {
            Ok((body, None))
        }
    }

    pub fn protocol(&self) -> RequestProtocol {
        self.ctx.protocol
    }

    pub fn content_type(&self) -> &'static str {
        self.ctx.protocol.response_content_type()
    }
}
```

### Feature Module Example: Compression

```rust
// ============================================================
// features/compression/mod.rs
// ============================================================

mod config;
mod encoding;
mod negotiate;
mod compress;
mod decompress;

pub use config::CompressionConfig;
pub use encoding::CompressionEncoding;
pub use negotiate::negotiate;
pub use compress::Compressor;
pub use decompress::Decompressor;

// ============================================================
// features/compression/negotiate.rs
// ============================================================

use super::{CompressionConfig, CompressionEncoding};
use crate::pipeline::CompressionContext;
use crate::error::{Code, ConnectError};
use axum::http::Request;

/// Negotiate compression from request headers.
/// Validates Content-Encoding and negotiates response encoding from Accept-Encoding.
pub fn negotiate<B>(
    req: &Request<B>,
    config: &CompressionConfig,
) -> Result<CompressionContext, ConnectError> {
    // Parse Content-Encoding
    let content_encoding = req
        .headers()
        .get(http::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());

    let request_encoding = CompressionEncoding::parse(content_encoding)
        .ok_or_else(|| {
            ConnectError::new(
                Code::Unimplemented,
                format!(
                    "unsupported compression \"{}\": supported encodings are gzip, identity",
                    content_encoding.unwrap_or("")
                ),
            )
        })?;

    // Parse Accept-Encoding and negotiate
    let accept_encoding = req
        .headers()
        .get(http::header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok());

    let response_encoding = CompressionEncoding::negotiate(accept_encoding, config);

    Ok(CompressionContext {
        request_encoding,
        response_encoding,
    })
}

// ============================================================
// features/compression/decompress.rs
// ============================================================

use super::CompressionEncoding;
use crate::error::{Code, ConnectError};
use bytes::Bytes;

pub struct Decompressor {
    encoding: CompressionEncoding,
}

impl Decompressor {
    pub fn new(encoding: CompressionEncoding) -> Self {
        Self { encoding }
    }

    pub fn decompress(&self, data: &Bytes) -> Result<Bytes, ConnectError> {
        match self.encoding {
            CompressionEncoding::Identity => Ok(data.clone()),
            CompressionEncoding::Gzip => {
                use flate2::read::GzDecoder;
                use std::io::Read;

                let mut decoder = GzDecoder::new(data.as_ref());
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed).map_err(|e| {
                    ConnectError::new(Code::InvalidArgument, format!("gzip decompression failed: {}", e))
                })?;
                Ok(Bytes::from(decompressed))
            }
        }
    }
}

// ============================================================
// features/compression/compress.rs
// ============================================================

use super::CompressionEncoding;
use crate::error::{Code, ConnectError};

pub struct Compressor {
    encoding: CompressionEncoding,
}

impl Compressor {
    pub fn new(encoding: CompressionEncoding) -> Self {
        Self { encoding }
    }

    pub fn encoding(&self) -> CompressionEncoding {
        self.encoding
    }

    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, ConnectError> {
        match self.encoding {
            CompressionEncoding::Identity => Ok(data.to_vec()),
            CompressionEncoding::Gzip => {
                use flate2::write::GzEncoder;
                use flate2::Compression;
                use std::io::Write;

                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(data).map_err(|e| {
                    ConnectError::new(Code::Internal, format!("gzip compression failed: {}", e))
                })?;
                encoder.finish().map_err(|e| {
                    ConnectError::new(Code::Internal, format!("gzip compression failed: {}", e))
                })
            }
        }
    }
}
```

### Integration Points

#### 1. ConnectLayer builds context only

```rust
// layer.rs

impl<S, ReqBody> Service<Request<ReqBody>> for ConnectService<S> {
    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Build context from request headers + config
        let context = match PipelineContext::from_request(&req, &self.config) {
            Ok(ctx) => ctx,
            Err(err) => {
                let response = err.into_response();
                return Box::pin(async move { Ok(response) });
            }
        };

        // Store context in extensions (pipelines reconstructed later)
        req.extensions_mut().insert(context);

        // Apply timeout wrapper
        let inner = self.inner.clone();
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move {
            match context.timeout.effective {
                Some(duration) => {
                    match tokio::time::timeout(duration, inner.oneshot(req)).await {
                        Ok(result) => result,
                        Err(_) => Ok(ConnectError::deadline_exceeded().into_response()),
                    }
                }
                None => inner.oneshot(req).await,
            }
        })
    }
}
```

#### 2. Request extractor reconstructs pipeline from context

```rust
// message/request.rs

impl<S, T> FromRequest<S> for ConnectRequest<T>
where
    T: Message + DeserializeOwned + Default,
{
    async fn from_request(req: Request, _state: &S) -> Result<Self, ConnectError> {
        // Get context from extensions
        let ctx = req
            .extensions()
            .get::<PipelineContext>()
            .ok_or_else(|| ConnectError::internal("missing pipeline context"))?;

        // Reconstruct request pipeline from context
        let pipeline = RequestPipeline::new(ctx);

        let body = Bytes::from_request(req, _state).await?;
        let message = pipeline.decode(body)?;

        Ok(ConnectRequest(message))
    }
}
```

#### 3. Handler wrapper reconstructs response pipeline from context

```rust
// handler.rs

// In ConnectHandlerWrapper::call:
let ctx = req
    .extensions()
    .get::<PipelineContext>()
    .copied()
    .unwrap_or_default();

// ... call handler ...

// Reconstruct response pipeline from context
let pipeline = ResponsePipeline::new(&ctx, DEFAULT_MIN_COMPRESS_BYTES);
match result {
    Ok(resp) => resp.into_response_with_pipeline(&pipeline),
    Err(err) => err.into_response_with_protocol(ctx.protocol),
}
```

#### 4. Response uses pipeline

```rust
// message/response.rs

impl<T: Message + Serialize> ConnectResponse<T> {
    pub fn into_response_with_pipeline(self, pipeline: &ResponsePipeline) -> Response {
        let (body, content_encoding) = match pipeline.encode(&self.0) {
            Ok(result) => result,
            Err(_) => return internal_error_response(),
        };

        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, pipeline.content_type());

        if let Some(encoding) = content_encoding {
            builder = builder.header(header::CONTENT_ENCODING, encoding);
        }

        builder.body(Body::from(body)).unwrap()
    }
}
```

### Data Flow Summary

```
┌─────────────────────────────────────────────────────────────────────────┐
│ ConnectLayer                                                            │
│   PipelineContext::from_request(&req, &config)                          │
│     → protocol::detect()                                                │
│     → compression::negotiate()                                          │
│     → timeout::parse()                                                  │
│   req.extensions_mut().insert(context)                                  │
│   Apply timeout wrapper around inner service                            │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ ConnectRequest Extractor                                                │
│   ctx = req.extensions().get::<PipelineContext>()                       │
│   pipeline = RequestPipeline::new(&ctx)                                 │
│   pipeline.decode(body)                                                 │
│     → compression::decompress()                                         │
│     → limits::check()                                                   │
│     → protocol::decode()                                                │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ User Handler                                                            │
│   async fn my_handler(req: ConnectRequest<T>) -> ConnectResponse<R>     │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ ConnectHandlerWrapper                                                   │
│   ctx = extensions.get::<PipelineContext>()                             │
│   pipeline = ResponsePipeline::new(&ctx, min_bytes)                     │
│   response.into_response_with_pipeline(&pipeline)                       │
│     → protocol::encode()                                                │
│     → compression::compress()                                           │
└─────────────────────────────────────────────────────────────────────────┘
```

### Benefits

1. **Feature isolation**: All compression code in `features/compression/`
2. **Pipeline composability**: `RequestPipeline` chains decompression → limits → decode
3. **Single context**: `PipelineContext` carries all negotiated state
4. **Testability**: Each feature module testable in isolation; pipelines testable end-to-end
5. **Extensibility**: Add new feature = add module + hook into pipeline builder

### Migration Path

1. Create `features/` modules (move existing code)
2. Create `pipeline/` with `PipelineConfig`, `PipelineContext`
3. Create `RequestPipeline::from_request()` that delegates to feature modules
4. Update `ConnectLayer` to use `RequestPipeline`
5. Create `ResponsePipeline` and update handlers
6. Remove scattered logic from old locations

---

## Alternative Approaches (for reference)

### Approach A: Feature Modules

Organize by feature, not by lifecycle stage. Each feature is a self-contained module:

```
src/
├── features/
│   ├── compression/
│   │   ├── mod.rs           # re-exports, feature orchestration
│   │   ├── config.rs        # CompressionConfig
│   │   ├── encoding.rs      # CompressionEncoding enum
│   │   ├── negotiate.rs     # header parsing, negotiation
│   │   ├── request.rs       # decompress_request()
│   │   └── response.rs      # compress_response()
│   │
│   ├── timeout/
│   │   ├── mod.rs
│   │   ├── config.rs        # TimeoutConfig (server max)
│   │   ├── parse.rs         # parse Connect-Timeout-Ms header
│   │   └── enforce.rs       # wrap_with_timeout() -> applies tokio::timeout
│   │
│   └── limits/
│       ├── mod.rs
│       ├── config.rs        # MessageLimits
│       └── check.rs         # check_size()
```

**Pros:**
- All compression code in one place
- Easy to understand a feature end-to-end
- Adding new feature = adding new directory

**Cons:**
- Still need glue code in layer.rs and handler.rs to call into features
- Features still run at different lifecycle points

---

### Approach B: Unified Context + Processors

Single context object carries all state. Processor traits define hooks:

```rust
/// All negotiated settings for a request.
/// Created once in layer, flows through entire lifecycle.
#[derive(Debug, Clone)]
pub struct ConnectContext {
    pub protocol: RequestProtocol,
    pub limits: MessageLimits,
    pub timeout: EffectiveTimeout,
    pub compression: CompressionContext,
}

/// Trait for request-side processing
pub trait RequestProcessor {
    fn process_request(&self, ctx: &ConnectContext, body: Bytes) -> Result<Bytes, ConnectError>;
}

/// Trait for response-side processing
pub trait ResponseProcessor {
    fn process_response(&self, ctx: &ConnectContext, body: Bytes) -> Result<(Bytes, HeaderMap), ConnectError>;
}
```

Then in layer.rs:
```rust
// Build context once
let ctx = ConnectContext::from_request(&req, &self.config)?;
req.extensions_mut().insert(ctx);
```

In request.rs:
```rust
let ctx = req.extensions().get::<ConnectContext>().unwrap();
let body = CompressionProcessor.process_request(ctx, raw_body)?;
```

**Pros:**
- Single source of truth for all settings
- Clear contract between lifecycle stages
- Easy to add new processors

**Cons:**
- More indirection
- Processor traits might be overkill

---

### Approach C: Pipeline Builder

Explicit pipeline that chains transforms:

```rust
pub struct RequestPipeline {
    decompressor: Option<Decompressor>,
    limit_checker: LimitChecker,
    decoder: Decoder,
}

pub struct ResponsePipeline {
    encoder: Encoder,
    compressor: Option<Compressor>,
}

impl RequestPipeline {
    /// Build pipeline from request headers and config
    pub fn from_request(req: &Request, config: &ConnectConfig) -> Result<Self, ConnectError> {
        let protocol = detect_protocol(req);
        let compression = negotiate_compression(req, config)?;

        Ok(Self {
            decompressor: compression.request.map(Decompressor::new),
            limit_checker: LimitChecker::new(config.limits),
            decoder: Decoder::new(protocol),
        })
    }

    /// Process raw body bytes into decoded message
    pub fn process<T: Message + DeserializeOwned>(&self, body: Bytes) -> Result<T, ConnectError> {
        let body = match &self.decompressor {
            Some(d) => d.decompress(body)?,
            None => body,
        };
        self.limit_checker.check(body.len())?;
        self.decoder.decode(body)
    }
}
```

Usage in handler:
```rust
let pipeline = req.extensions().get::<RequestPipeline>().unwrap();
let message: MyRequest = pipeline.process(body)?;
```

**Pros:**
- Very explicit about what happens
- Pipeline is testable in isolation
- No scattered logic - pipeline owns the full transform

**Cons:**
- Requires restructuring extractors
- More complex types

---

### Approach D: Configuration + Context Split (Minimal Change)

Keep current structure but clarify separation:

1. **`ConnectConfig`** - Static server configuration (lives in layer)
   ```rust
   pub struct ConnectConfig {
       pub limits: MessageLimits,
       pub server_timeout: Option<Duration>,
       pub compression: CompressionConfig,
       pub require_protocol_header: bool,
   }
   ```

2. **`ConnectContext`** - Per-request negotiated state (stored in extensions)
   ```rust
   pub struct ConnectContext {
       pub protocol: RequestProtocol,
       pub effective_timeout: Option<Duration>,
       pub request_encoding: CompressionEncoding,
       pub response_encoding: CompressionEncoding,
   }
   ```

3. **Feature functions** - Stateless functions that take context
   ```rust
   // compression.rs
   pub fn decompress_body(ctx: &ConnectContext, body: Bytes) -> Result<Bytes, ConnectError>;
   pub fn compress_body(ctx: &ConnectContext, config: &ConnectConfig, body: &[u8]) -> (Vec<u8>, Option<&'static str>);

   // timeout.rs
   pub fn parse_and_compute_timeout(req: &Request, server_timeout: Option<Duration>) -> Option<Duration>;
   ```

**Pros:**
- Minimal restructuring
- Clear separation: config vs context vs functions
- Functions are easy to test

**Cons:**
- Still need to call functions from multiple places
- Less "pipeline" feel

---

## Recommendation

**Start with Approach D** (minimal change), then evolve toward **Approach A** (feature modules) if the codebase grows.

Immediate refactor:
1. Create `ConnectContext` that consolidates all per-request state
2. Move scattered functions into their feature's module
3. Keep layer.rs as the orchestrator that builds context
4. Keep handler.rs as glue that passes context to response

This gives better organization without major restructuring.

---

## Questions to Decide

1. Should `ConnectContext` be one struct or separate structs per feature?
2. Should timeout enforcement stay in layer.rs or move to a wrapper function?
3. How should streaming vs unary compression contexts differ?
