//! Example: Idempotency Level - Auto GET Support
//!
//! This example demonstrates automatic GET request support for methods marked with
//! `idempotency_level = NO_SIDE_EFFECTS` in the proto definition.
//!
//! The `GetGreeting` method in hello.proto is marked as:
//!   rpc GetGreeting(HelloRequest) returns (HelloResponse) {
//!     option idempotency_level = NO_SIDE_EFFECTS;
//!   }
//!
//! The code generator automatically enables both GET and POST for this method.
//! No manual `get_connect().merge(post_connect())` is needed!
//!
//! Run with: cargo run --bin idempotency-get
//! Test with:
//!   GET:  curl "http://localhost:3000/hello.HelloWorldService/GetGreeting?connect=v1&encoding=json&message=%7B%22name%22%3A%22Alice%22%7D"
//!   POST: curl -X POST http://localhost:3000/hello.HelloWorldService/GetGreeting -H 'Content-Type: application/json' -d '{"name": "Alice"}'

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{helloworldservice, HelloRequest, HelloResponse};
// SocketAddr now provided by server_addr()

/// Handler for the idempotent GetGreeting RPC.
/// Works with both GET and POST requests automatically.
async fn get_greeting(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Greetings, {}! (via auto-enabled GET)", name),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Using the service builder - GET is automatically enabled for GetGreeting
    // because it's marked with `idempotency_level = NO_SIDE_EFFECTS` in the proto
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .get_greeting(get_greeting)
        .build_connect();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Idempotency Level: Auto GET Support ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - GetGreeting: GET+POST /hello.HelloWorldService/GetGreeting");
    println!("    (auto-enabled because idempotency_level = NO_SIDE_EFFECTS)");
    println!();
    println!("Test GET request:");
    println!(
        "  curl \"http://localhost:3000/hello.HelloWorldService/GetGreeting?connect=v1&encoding=json&message=%7B%22name%22%3A%22Alice%22%7D\""
    );
    println!();
    println!("Test POST request:");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/GetGreeting \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    axum::serve(listener, router).await?;
    Ok(())
}
