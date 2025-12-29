//! Example 1: Pure ConnectRPC Unary
//!
//! This example demonstrates the simplest ConnectRPC setup:
//! - Unary RPC (single request, single response)
//! - No gRPC support (pure Connect protocol)
//! - Stateless handlers
//!
//! Run with: cargo run --bin connect-unary --no-default-features
//! Test with Go client: go run ./cmd/client --protocol connect unary

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use std::net::SocketAddr;

/// Simple stateless handler for unary RPC
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build_connect();

    // MakeServiceBuilder applies ConnectLayer for protocol detection
    // let app = connectrpc_axum::MakeServiceBuilder::new()
    //     .add_router(router)
    //     .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example 1: Pure ConnectRPC Unary ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHello (unary): POST /hello.HelloWorldService/SayHello");
    println!();
    println!("Test with:");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    axum::serve(listener, router).await?;
    Ok(())
}
