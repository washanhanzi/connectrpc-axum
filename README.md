# ConnectRPC Axum

Axum-style Connect Protocol Server.

> Work in progress.

## 1 Code Generation (build.rs)

Add a build script to generate code from your `.proto` files.

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // EITHER: Connect-only
    // connectrpc_axum_build::compile_dir("proto").compile()?;

    // OR: Connect + Tonic (enable the "tonic" feature on the build crate)
    connectrpc_axum_build::compile_dir("proto").with_tonic().compile()?;
    Ok(())
}
```

## 2 Connect Server

Use any number of `FromRequestParts` extractors first, and end with `ConnectRequest<T>`.

```rust
use axum::{extract::{Query, State}, Router};
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

#[derive(serde::Deserialize)]
struct Pagination { page: usize, per_page: usize }

// Multiple extractors + Connect body
async fn say_hello(
    Query(_p): Query<Pagination>,
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hello, {}!", req.name.unwrap_or_default()) }))
}

// Minimal handler (no state)
async fn say_hello_simple(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hi, {}!", req.name.unwrap_or_default()) }))
}

// Build routes via the generated service builder (no manual paths)
let router = helloworldservice::HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .say_hello_simple(say_hello_simple)
    .with_state(AppState::default())
    .build();

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, tower::make::Shared::new(router)).await?;
```

## 3 Tonic-compatible Server

Enable features in `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum = { version = "*", features = ["tonic"] }

[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

build.rs:

```rust
connectrpc_axum_build::compile_dir("proto").with_tonic().compile()?;
```

Use the generated Tonic-compatible builder and single-port dispatcher:

```rust
use connectrpc_axum::prelude::*;

#[derive(Clone, Default)]
struct AppState;

// Tonic-compatible handler signatures (only these two compile):
// 1) (ConnectRequest<Req>)
// 2) (State<S>, ConnectRequest<Req>)
async fn say_hello(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hello, {}!", req.name.unwrap_or_default()) }))
}

let (router, svc) = helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
    .say_hello(say_hello)
    .with_state(AppState::default())
    .build();

let grpc = hello_world_service_server::HelloWorldServiceServer::new(svc);

let dispatch = connectrpc_axum::ContentTypeSwitch::new(grpc, router);
axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
```

Constraints in Tonic-compatible mode
- Allowed only:
  - `(ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>`
  - `(State<S>, ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>`
- In Connect-only mode, any number of extractors is allowed before `ConnectRequest<Req>`.

## Thanks to

- [AThilenius/axum-connect](https://github.com/AThilenius/axum-connect)
- [tokio-rs/axum](https://github.com/tokio-rs/axum)