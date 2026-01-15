//! Example: GET Request Support
//!
//! This example demonstrates GET request support for idempotent unary RPCs.
//! GET requests encode the message in query parameters:
//! - `connect=v1` (protocol version)
//! - `encoding=json|proto` (message encoding)
//! - `message=<payload>` (URL-encoded message)
//! - `base64=1` (optional, for binary payloads)
//! - `compression=gzip|identity` (optional)
//!
//! Run with: cargo run --bin get-request
//! Test with:
//!   GET: curl "http://localhost:3000/hello.HelloWorldService/SayHello?connect=v1&encoding=json&message=%7B%22name%22%3A%22Alice%22%7D"
//!   POST: curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello -H 'Content-Type: application/json' -d '{"name": "Alice"}'

use axum::Router;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse};
use std::net::SocketAddr;

/// Handler for unary RPC - works with both GET and POST
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
    // Build router with GET and POST support
    // Users combine get_connect and post_connect as needed
    let router = Router::new()
        .route(
            "/hello.HelloWorldService/SayHello",
            get_connect(say_hello).merge(post_connect(say_hello)),
        )
        .layer(ConnectLayer::new());

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== GET Request Support Example ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHello: GET/POST /hello.HelloWorldService/SayHello");
    println!();
    println!("Test GET request:");
    println!(
        "  curl \"http://localhost:3000/hello.HelloWorldService/SayHello?connect=v1&encoding=json&message=%7B%22name%22%3A%22Alice%22%7D\""
    );
    println!();
    println!("Test POST request:");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    axum::serve(listener, router).await?;
    Ok(())
}
