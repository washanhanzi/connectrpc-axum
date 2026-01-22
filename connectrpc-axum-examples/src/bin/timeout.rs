//! Example: Connect-Timeout-Ms header testing
//!
//! This example demonstrates timeout enforcement:
//! - Handler sleeps for 500ms before responding
//! - Clients can set Connect-Timeout-Ms header
//! - Requests with timeout < 500ms will get deadline_exceeded error
//! - Requests with timeout >= 500ms or no timeout will succeed
//!
//! **Important**: Using axum's `TimeoutLayer` will NOT give you Connect protocol
//! timeout behavior. To properly handle `Connect-Timeout-Ms` headers, you must use
//! either:
//! - The `.timeout()` method on `MakeServiceBuilder`
//! - The `ConnectTimeoutLayer` directly
//!
//! Run with: cargo run --bin timeout
//! Test with Go client: go run ./cmd/client timeout

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use std::net::SocketAddr;
use std::time::Duration;

/// Handler that sleeps for 500ms before responding
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    // Sleep for 500ms to simulate slow processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! (after 500ms delay)", name),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    // MakeServiceBuilder applies ConnectLayer for protocol detection
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example: Connect-Timeout-Ms Testing ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Handler sleeps for 500ms before responding.");
    println!();
    println!("Test with:");
    println!("  # Should succeed (no timeout):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");
    println!();
    println!("  # Should fail (100ms timeout < 500ms handler):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -H 'Connect-Timeout-Ms: 100' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    axum::serve(listener, app).await?;
    Ok(())
}
