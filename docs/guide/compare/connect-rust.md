---
title: Comparison with connect-rust
repo: https://github.com/connectrpc/connect-rust
commit: 0b9e25a837c05f469ce4a1a54b70e0ac20fff809
date: 2026-07-08
author: Claude Fable 5
---

# Comparison: connectrpc-axum vs connect-rust

<ComparisonMeta />

## Overview

[connect-rust](https://github.com/connectrpc/connect-rust) is the official ConnectRPC Rust implementation, published as the `connectrpc` crate (v0.8.1 at the time of writing). It is a self-contained, Tower-based RPC runtime that natively serves all three protocols — Connect, gRPC over HTTP/2, and gRPC-Web — from a single dispatcher, and passes the full ConnectRPC conformance suite (3,600 server and 6,872 client tests). It uses [buffa](https://github.com/anthropics/buffa) rather than prost for protobuf, gaining zero-copy message views where string fields borrow directly from the request buffer.

The two projects answer the same question — "how do I serve Connect RPCs from Rust?" — with opposite integration philosophies. connectrpc-axum embeds the Connect protocol *into* Axum's programming model: handlers are ordinary axum handler functions with extractors, routes are axum `Router`s, and gRPC support is delegated to tonic. connect-rust is framework-*agnostic*: it implements its own router, dispatcher, and protocol stack behind a `tower::Service`, and treats Axum as just one possible host (via `into_axum_service()`), alongside raw hyper or its own built-in server.

## Feature Comparison

### Protocols & Encodings

**connectrpc-axum:**
- Connect protocol (unary POST + GET for idempotent methods, all four streaming types) implemented natively
- JSON and binary protobuf via prost + pbjson
- gRPC via optional tonic integration, gRPC-Web via tonic-web, multiplexed on one port by `Content-Type`

**connect-rust:**
- Connect, gRPC over HTTP/2, and gRPC-Web all implemented natively in one runtime
- JSON and binary protobuf via buffa (JSON is a default feature that can be compiled out for proto-only builds)
- Unary GET for idempotent methods
- Passes the full ConnectRPC conformance suite across all three protocols

The largest difference is gRPC strategy: connect-rust implements gRPC/gRPC-Web itself with one shared dispatch path, while connectrpc-axum routes `application/grpc*` traffic to a tonic server. connect-rust's conformance coverage is comprehensively verified in CI; connectrpc-axum validates against connect-go interop tests.

### Streaming

**connectrpc-axum:**
- All four RPC types via extractor/response types: `ConnectRequest<Streaming<Req>>` for inbound streams, `ConnectResponse<StreamBody<St>>` for outbound
- Client library supports all four types with `Streaming<T>` response decoding

**connect-rust:**
- All four RPC types via generated trait methods: `InboundStream<Req>` (a stream of zero-copy `StreamMessage<Req>` items) inbound, `ServiceStream<Res>` outbound
- Inbound streams surface transport failures as `Err` items so a truncated upload is never mistaken for a clean end-of-stream
- Client streaming handles expose `send()` / `message()` / `close_send()` for bidi
- Stream-message batching into fewer HTTP/2 DATA frames as a performance optimization

Feature parity is essentially equal here; the difference is idiom (axum extractor types vs generated trait signatures with zero-copy message wrappers).

### Compression & Performance

**connectrpc-axum:**
- Gzip, deflate, brotli, and zstd codecs
- Unary RPCs reuse tower-http's `RequestDecompressionLayer` / `CompressionLayer`; streaming RPCs get per-envelope compression with `Connect-Accept-Encoding` negotiation
- Decompression bounded by `receive_max_bytes` to prevent decompression bombs

**connect-rust:**
- Gzip and zstd by default, plus a pluggable `CompressionRegistry` for custom algorithms
- Streaming compression via async-compression
- Extensive published benchmarks: ~2× lower unary latency than tonic, and zero-copy decode advantages on decode-heavy workloads (buffa views avoid per-string allocations and HashMap materialization)

connectrpc-axum supports more built-in algorithms; connect-rust offers a pluggable registry and invests heavily in measured performance (compile-time match dispatch, two-frame unary bodies, zero-allocation string access).

### Error Handling

**connectrpc-axum:**
- `ConnectError` with Connect error codes, structured error details, and protocol-appropriate serialization

**connect-rust:**
- `ConnectError` with `ErrorCode`, message, structured details, and metadata (headers + trailers)
- The dispatcher serializes the error per negotiated protocol (Connect JSON/binary, gRPC trailers, gRPC-Web), so handlers are protocol-unaware

Both follow the connect-go error model closely; connect-rust additionally carries error metadata and has three wire formats to serialize into.

## API Design

The API surfaces reflect the two integration philosophies directly.

**connectrpc-axum** — handlers are free functions using axum's extractor pattern; generated builders produce an axum `Router`. Any `FromRequestParts` extractor (state, headers, connection info) can precede the `ConnectRequest`:

```rust
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
        response_type: None,
    }))
}

let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .build_connect();
axum::serve(listener, router).await?;
```

**connect-rust** — services are generated traits implemented on a struct; requests arrive as borrowed zero-copy views, and the router is mounted into axum as a fallback service:

```rust
impl GreetService for MyGreetService {
    async fn greet(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, GreetRequest>,
    ) -> ServiceResult<GreetResponse> {
        // req.name is a &str borrowed from the request buffer (zero-copy)
        Response::ok(GreetResponse {
            greeting: format!("Hello, {}!", req.name),
            ..Default::default()
        })
    }
}

let connect = ConnectRouter::new().add_service(Arc::new(MyGreetService));
let app = Router::new().fallback_service(connect.into_axum_service());
```

connectrpc-axum optimizes for developers already living in axum: extractors, middleware, and state work exactly as they do for REST routes, and a tonic-compatible trait style is also available. connect-rust optimizes for protocol fidelity and performance: the trait-based model mirrors connect-go, the `RequestContext` / `Response<B>` split gives typed access to deadlines, peer certs, and trailers, and borrowed views eliminate decode allocations.

Two notable connect-rust capabilities without a direct connectrpc-axum server-side equivalent:

- **Server interceptors** — a typed, async per-RPC middleware layer (`Interceptor` trait with `Next`, matching connect-go's `WithInterceptors`) that runs after protocol parsing with access to the resolved `Spec`, headers, deadline, and a lazily decoded message body. connectrpc-axum has a comparable interceptor system on the *client* only; on the server it relies on tower/axum middleware, which operates below the RPC layer.
- **Ecosystem services** — first-party `connectrpc-health` (grpc.health.v1) and `connectrpc-reflection` (grpcurl/buf curl discovery) crates, plus built-in TLS/mTLS, a standalone hyper server, and wasm client support via a generic `ClientTransport`.

## Implementation Details

| Aspect | connectrpc-axum | connect-rust |
|--------|-----------------|--------------|
| Architecture | Axum layers/extractors | Tower service + own router |
| Proto library | prost (+ pbjson) | buffa (zero-copy views) |
| gRPC / gRPC-Web | Via tonic / tonic-web | Native |
| Request handling | Middleware stack + extractors | Dispatcher + generated match |
| Middleware | tower/axum layers | Tower layers + RPC interceptors |
| Code generation | build.rs (prost/pbjson/tonic passes) | protoc plugin or build.rs (quote! + prettyplease) |
| Dispatch | Axum routing per method | Compile-time match, no `Arc<dyn Handler>` vtable |

connect-rust supports two codegen workflows — a `protoc-gen-connect-rust` plugin for `buf generate` with checked-in output, and a `connectrpc-build` build.rs path — whereas connectrpc-axum is build.rs-only with a multi-stage prost/pbjson/tonic pipeline unified by `extern_path`.

## Summary

**connectrpc-axum strengths:**
- Deep axum-native ergonomics: any `FromRequestParts` extractor, axum state, and axum middleware work in RPC handlers unchanged
- `MakeServiceBuilder` composes Connect services, tonic gRPC services, and plain axum routes into one app on one port
- Reuses the mature tonic/tower-http ecosystem instead of reimplementing gRPC and HTTP compression
- Broader built-in compression codec set (gzip, deflate, brotli, zstd)
- Typed client interceptor chain with zero-cost composition (no dynamic dispatch)

**connect-rust strengths:**
- Full conformance-suite verification (3,600 server + 6,872 client tests) across Connect, gRPC, and gRPC-Web
- Zero-copy request views via buffa, with published benchmarks showing material wins over tonic on decode-heavy workloads
- Server-side typed RPC interceptors equivalent to connect-go's `WithInterceptors`
- First-party health and reflection services, TLS/mTLS, standalone server, and wasm client support
- Framework-agnostic tower core usable outside axum

**Key takeaways:**
- A server-side RPC-level interceptor layer (typed access to procedure, headers, deadline, and lazily decoded messages) is the clearest feature gap; today users must drop to tower middleware, which cannot see decoded messages
- Running the official ConnectRPC conformance suite in CI would substantiate protocol correctness the way connect-rust does, and would likely surface edge cases (the recent invalid-flags/end-stream fixes suggest this is already a direction)
- First-party health and reflection crates are cheap, high-leverage ecosystem additions worth considering; connect-rust's `Payload` lazy-decode-cache pattern (interceptor decodes once, handler reuses it) is also worth studying if server interceptors are added
