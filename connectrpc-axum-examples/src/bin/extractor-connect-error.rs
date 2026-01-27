//! Example: Extractor rejection with ConnectError
//!
//! This example demonstrates an axum extractor that rejects with `ConnectError`.
//! When the extractor returns a `ConnectError`, the `handle_extractor_rejection`
//! function (handler.rs:25) encodes it using the Connect protocol, preserving
//! proper gRPC status codes and error details.
//!
//! Run with: cargo run --bin extractor-connect-error --no-default-features
//! Test with:
//!   # Without header (should fail with UNAUTHENTICATED)
//!   curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \
//!     -H 'Content-Type: application/json' \
//!     -d '{"name": "Alice"}'
//!
//!   # With header (should succeed)
//!   curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \
//!     -H 'Content-Type: application/json' \
//!     -H 'x-user-id: user123' \
//!     -d '{"name": "Alice"}'

use axum::{extract::FromRequestParts, http::request::Parts};
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
// SocketAddr now provided by server_addr()

/// Custom extractor that validates the x-user-id header.
/// Returns `ConnectError` on rejection, which gets encoded with Connect protocol.
pub struct UserId(pub String);

impl<S> FromRequestParts<S> for UserId
where
    S: Send + Sync,
{
    type Rejection = ConnectError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| UserId(s.to_string()))
            .ok_or_else(|| ConnectError::new_unauthenticated("missing x-user-id header"))
    }
}

/// Handler that uses the UserId extractor
async fn say_hello(
    UserId(user_id): UserId,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! (authenticated as {})", name, user_id),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build_connect();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example: Extractor Rejection with ConnectError ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("This example demonstrates handle_extractor_rejection (handler.rs:25)");
    println!("When extractor returns ConnectError, it's encoded with Connect protocol.");
    println!();
    println!("Test WITHOUT x-user-id header (should fail with UNAUTHENTICATED):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");
    println!();
    println!("Test WITH x-user-id header (should succeed):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -H 'x-user-id: user123' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    axum::serve(listener, router).await?;
    Ok(())
}
