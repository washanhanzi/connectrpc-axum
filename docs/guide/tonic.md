# Tonic gRPC Integration

Serve both Connect and gRPC clients on the same port using Tonic integration.

## Installation

Add the `tonic` feature to your dependencies:

```toml
[dependencies]
connectrpc-axum = { version = "*", features = ["tonic"] }
tonic = "0.14"
futures = "0.3"
tower = "0.5"

[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

## Update build.rs

Enable Tonic code generation:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()  // Enable Tonic gRPC code generation
        .compile()?;
    Ok(())
}
```

## Use TonicCompatibleBuilder

The `TonicCompatibleBuilder` generates both Connect router and gRPC service from the same handlers:

```rust
use axum::extract::State;
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

// Tonic-compatible handlers only allow:
// - (ConnectRequest<Req>)
// - (State<S>, ConnectRequest<Req>)
async fn say_hello(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build both Connect router and gRPC server from same handlers
    let (connect_router, grpc_server) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .with_state(AppState::default())
            .build();

    // Combine into a single service that routes by Content-Type:
    // - application/grpc* -> Tonic gRPC server
    // - Otherwise -> Axum routes (Connect protocol)
    let service = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, tower::make::Shared::new(service)).await?;
    Ok(())
}
```

## Handler Restrictions

Tonic-compatible handlers have restricted signatures to ensure compatibility with both protocols:

| Signature | State Type |
|-----------|------------|
| `(ConnectRequest<Req>)` | `()` |
| `(State<S>, ConnectRequest<Req>)` | Generic `S` |

::: warning
Other Axum extractors (like `Query`, `Path`, etc.) are not supported in Tonic-compatible handlers since gRPC doesn't have equivalent concepts.
:::

## Request Routing

Requests are routed by `Content-Type` header:

- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum (Connect protocol)