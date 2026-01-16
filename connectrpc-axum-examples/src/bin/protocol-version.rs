//! Protocol Version Validation Example
//!
//! This example demonstrates the `require_protocol_header` feature:
//! - Requires `Connect-Protocol-Version: 1` header on Connect protocol requests
//! - Returns `invalid_argument` error when header is missing or wrong
//!
//! Run with: cargo run --bin protocol-version
//! Test with Go client: go run ./cmd/client protocol-version

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use std::net::SocketAddr;

/// Simple handler for testing protocol version validation
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! (protocol version validated)", name),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    // MakeServiceBuilder applies ConnectLayer with require_protocol_header
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .require_protocol_header(true)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Protocol Version Validation Example ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("This server requires Connect-Protocol-Version: 1 header");
    println!();
    println!("Test with:");
    println!("  # Should succeed (with header):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -H 'Connect-Protocol-Version: 1' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");
    println!();
    println!("  # Should fail (missing header):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Bob\"}}'");
    println!();
    println!("  # Should fail (wrong version):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -H 'Connect-Protocol-Version: 2' \\");
    println!("    -d '{{\"name\": \"Charlie\"}}'");

    axum::serve(listener, tower::make::Shared::new(app)).await?;
    Ok(())
}
