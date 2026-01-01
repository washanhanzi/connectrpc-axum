# gRPC-Web Support

Enable browser-based clients to call gRPC services using gRPC-Web protocol with `tonic-web`.

## Installation

Add the `tonic` feature and `tonic-web` dependency:

```toml
[dependencies]
connectrpc-axum = { version = "*", features = ["tonic"] }
tonic = "0.14"
tonic-web = "0.14"

[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

## Usage

Wrap the gRPC server with `tonic-web` layer:

```rust
use axum::extract::State;
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

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

    // Wrap gRPC server with gRPC-Web layer
    let grpc_web_server = tower::ServiceBuilder::new()
        .layer(tonic_web::GrpcWebLayer::new())
        .service(grpc_server);

    // Combine with MakeServiceBuilder
    let service = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_web_server)
        .build();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, tower::make::Shared::new(service)).await?;
    Ok(())
}
```

## Request Routing

Requests are routed by `Content-Type` header:

- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum (Connect protocol)
