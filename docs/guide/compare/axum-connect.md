# Comparison: connectrpc-axum vs axum-connect

## Overview

[axum-connect](https://github.com/AThilenius/axum-connect) brings the Connect protocol to Axum with an ergonomic API that mirrors Axum's extractor patterns. Created by Alec Thilenius, the latest release is v0.5.3.

## Feature Comparison

| Feature | connectrpc-axum | axum-connect |
|---------|-----------------|--------------|
| Unary RPC (POST) | :white_check_mark: | :white_check_mark: |
| Unary RPC (GET) | :white_check_mark: | :white_check_mark: |
| Server streaming | :white_check_mark: | :white_check_mark: |
| Client streaming | :white_check_mark: | :x: |
| Bidirectional streaming | :white_check_mark: | :x: |
| JSON encoding | :white_check_mark: | :white_check_mark: |
| Protobuf encoding | :white_check_mark: | :white_check_mark: |
| Compression (gzip) | :white_check_mark: | :x: |
| Timeouts | :white_check_mark: | :x: (TODO) |
| Message size limits | :white_check_mark: | :x: |
| Error details | :white_check_mark: | :x: |
| Tonic/gRPC interop | :white_check_mark: (optional) | :x: |
| Auto protoc download | :x: | :white_check_mark: |

## API Comparison

**connectrpc-axum:**
```rust
async fn say_hello(
    req: ConnectRequest<HelloRequest>,
) -> ConnectResponse<HelloResponse> {
    ConnectResponse::success(HelloResponse { message: "Hello".into() })
}

HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .build_connect()
```

**axum-connect:**
```rust
async fn say_hello(
    Host(host): Host,
    request: HelloRequest,
) -> Result<HelloResponse, Error> {
    Ok(HelloResponse { message: "Hello".into() })
}

Router::new().rpc(HelloWorldService::say_hello(say_hello))
```

## Key Technical Differences

- **Middleware vs inline**: connectrpc-axum uses a Tower layer (`ConnectLayer`) for protocol handling; axum-connect processes everything inline in generated handlers.

- **Explicit vs ergonomic types**: connectrpc-axum wraps requests/responses in `ConnectRequest<T>`/`ConnectResponse<T>` for protocol awareness; axum-connect uses raw protobuf types directly.

- **Multi-service composition**: connectrpc-axum's `MakeServiceBuilder` composes multiple services with shared configuration; axum-connect chains `.rpc()` calls on a single router.

- **Error model**: connectrpc-axum supports full Connect error details (additional proto messages); axum-connect has basic error codes only.

## Handler Transformation

Both libraries transform user functions into axum-compatible handlers, but use different approaches:

**axum-connect** uses a macro-based approach:
- Separate traits for each RPC pattern: `RpcHandlerUnary`, `RpcHandlerStream`
- `impl_handler!` macro generates trait implementations for each (0-15 extractor parameters)
- Defines custom `RpcFromRequestParts` trait (mirrors axum's `FromRequestParts`)
- Custom `RpcIntoResponse` trait for response handling

**connectrpc-axum** uses a newtype wrapper approach:
- Unified `ConnectHandlerWrapper<F>` handles all RPC patterns (unary, server/client/bidi streaming)
- Compiler selects impl based on handler signature via trait bounds
- Reuses axum's native `FromRequestParts` directly (no custom trait)

The key difference: axum-connect requires extractors to implement their RPC-specific traits and separates handler logic by RPC type, while connectrpc-axum uses a single unified wrapper that works with any standard axum extractor.

## Summary

| connectrpc-axum strengths | axum-connect strengths |
|---------------------------|------------------------|
| Full streaming support (client + bidi) | More ergonomic handler signatures |
| Tower middleware integration | Simpler learning curve |
| Compression, timeouts, limits | Auto protoc fetching |
| Error details support | Less boilerplate for simple cases |
| Optional Tonic/gRPC interop | Automatic GET variant generation |
