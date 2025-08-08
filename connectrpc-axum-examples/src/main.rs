use axum::extract::Query;
use axum::extract::State;
use connectrpc_axum::prelude::*;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::AtomicUsize};
// use tower::ServiceExt;

// Using generated code directly from OUT_DIR (recommended pattern)
include!(concat!(env!("OUT_DIR"), "/hello.rs"));

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}
#[derive(Deserialize)]
struct Pagination {
    page: usize,
    per_page: usize,
}

// Tonic-compatible handler functions that work with both Connect and gRPC
async fn say_hello(
    Query(_pagination): Query<Pagination>,
    State(_app_state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

async fn say_hello_stream(
    State(_app_state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Stream Hello, {}!", req.name.unwrap_or_default()),
    }))
}

// Example of a stateless handler (also Tonic-compatible)
async fn say_hello_simple(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Simple Hello, {}!", req.name.unwrap_or_default()),
    }))
}

// Example of a stateful handler (Tonic-compatible)
async fn say_hello_with_state(
    State(_state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Stateful Hello, {}!", req.name.unwrap_or_default()),
    }))
}

// Tonic trait impl is generated; no manual impl needed here.

// Dispatcher moved into library: connectrpc_axum::ContentTypeSwitch

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    let _router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .with_state(app_state.clone())
        .build();

    let _router_without_state = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello_simple)
        .build();

    // Build Connect routes and gRPC service using the same handlers
    let (connect_router, grpc_svc) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello_with_state)
            .say_hello_stream(say_hello_stream)
            .with_state(app_state)
            .build();

    // Build the dispatch service (no Arc needed)
    let dispatch = connectrpc_axum::ContentTypeSwitch::new(grpc_svc, connect_router);

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("listening on http://{}", addr);

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
