//! Example 3: Tonic gRPC Unary (also works with ConnectRPC)
//!
//! This example demonstrates dual-protocol support:
//! - Unary RPC works with both Connect and gRPC protocols
//! - Uses TonicCompatibleBuilder to generate both routers
//! - Same handler serves both protocols
//!
//! Run with: cargo run --bin tonic-unary
//! Test with:
//!   - Connect: go run ./cmd/client --protocol connect unary
//!   - gRPC:    go run ./cmd/client --protocol grpc unary

use axum::extract::State;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use std::net::SocketAddr;
use std::sync::{Arc, atomic::AtomicUsize};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

/// Handler with state - works for both Connect and gRPC
async fn say_hello(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = req.name.unwrap_or_else(|| "World".to_string());

    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello #{}, {}!", count, name),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build both Connect router and gRPC server from same handlers
    let (connect_router, grpc_server) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .with_state(app_state)
            .build();

    // Combine into a single service that routes by Content-Type
    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example 3: Tonic gRPC Unary (+ ConnectRPC) ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHello (unary): /hello.HelloWorldService/SayHello");
    println!();
    println!("Protocols supported:");
    println!("  - Connect (JSON): Content-Type: application/json");
    println!("  - Connect (Proto): Content-Type: application/proto");
    println!("  - gRPC: Content-Type: application/grpc");
    println!();
    println!("Test with:");
    println!("  go run ./cmd/client --protocol connect unary");
    println!("  go run ./cmd/client --protocol grpc unary");

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
