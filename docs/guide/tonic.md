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

// Tonic-compatible handlers accept FromRequestParts extractors:
// - (ConnectRequest<Req>)
// - (Extractor1, ..., ConnectRequest<Req>) - up to 8 extractors
async fn say_hello(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse::new(HelloResponse {
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

## Server Streaming

Server streaming handlers work the same way:

```rust
use futures::Stream;

async fn say_hello_stream(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let response_stream = async_stream::stream! {
        yield Ok(HelloResponse {
            message: format!("Hello, {}!", name),
        });
        yield Ok(HelloResponse {
            message: format!("Goodbye, {}!", name),
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}
```

## Request Routing

Requests are routed by `Content-Type` header:

- `application/grpc*` → Tonic gRPC server (includes gRPC-Web)
- Otherwise → Axum (Connect protocol)