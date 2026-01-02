# Comparison: connectrpc-axum vs connectrpc

## Overview

[connectrpc](https://github.com/nikola-jokic/connectrpc) is a Rust implementation of the Connect protocol that provides both client and server components. It includes a Reqwest-based client and an Axum server implementation, with code generation for both. The project is in early development with streaming support being actively developed.

## Feature Comparison

| Feature | connectrpc-axum | connectrpc |
|---------|-----------------|------------|
| Unary RPC (POST) | :white_check_mark: | :white_check_mark: |
| Unary RPC (GET) | :white_check_mark: | :white_check_mark: |
| Server streaming | :white_check_mark: | :white_check_mark: |
| Client streaming | :white_check_mark: | :white_check_mark: |
| Bidirectional streaming | :white_check_mark: | Planned |
| JSON encoding | :white_check_mark: | :white_check_mark: |
| Protobuf encoding | :white_check_mark: | :white_check_mark: |
| Compression (gzip) | :white_check_mark: | :x: |
| Timeouts | :white_check_mark: | :white_check_mark: |
| Error details | :white_check_mark: | :white_check_mark: |
| Message size limits | :white_check_mark: | :x: |
| Tonic/gRPC interop | :white_check_mark: | :x: |
| gRPC-Web | :white_check_mark: | :x: |
| Generated client | :x: | :white_check_mark: (Reqwest) |

## API Comparison

**connectrpc-axum:**
```rust
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .build_connect()
```

**connectrpc:**
```rust
async fn say_hello(
    state: State,
    req: UnaryRequest<HelloRequest>,
) -> Result<UnaryResponse<HelloResponse>> {
    Ok(UnaryResponse::new(HelloResponse {
        message: format!("Hello, {}!", req.into_message().name.unwrap_or_default()),
    }))
}

HelloWorldServiceAxumServer {
    state: my_state,
    say_hello,
}.into_router()
```

## Key Technical Differences

- **Architecture**: connectrpc-axum uses middleware (`ConnectLayer`) and Axum extractors for protocol handling, while connectrpc parses requests inline within handler trait implementations.

- **State handling**: connectrpc-axum leverages Axum's native state management via `State<T>` extractor. connectrpc requires state to be passed explicitly as a field on the generated server struct.

- **Client generation**: connectrpc generates Reqwest-based clients (Proto and JSON variants) at build time. connectrpc-axum is server-only and relies on existing Connect clients.

- **gRPC support**: connectrpc-axum provides `ContentTypeSwitch` for routing between Connect and gRPC (Tonic) services. connectrpc is Connect-only with no gRPC interop.

- **Code output**: connectrpc writes generated code to user-specified files (e.g., `src/lib.rs`). connectrpc-axum generates to `OUT_DIR` and uses `include!` macros.

## Summary

| connectrpc-axum strengths | connectrpc strengths |
|---------------------------|----------------------|
| Full streaming support (including bidi) | Generated Reqwest client |
| Compression support | Flexible output file placement |
| Tonic/gRPC interop | Simpler standalone library |
| Message size limits | Combined client+server in one crate |
| Idiomatic Axum extractors | - |
