//! Example: Extractor rejection with plain HTTP response
//!
//! This example demonstrates an axum extractor that rejects with a plain HTTP response.
//! When the extractor returns a non-ConnectError response (like HTTP 401 or 302 redirect),
//! the `handle_extractor_rejection` function (handler.rs:25) returns it as-is, bypassing
//! Connect protocol encoding. This is useful for authentication flows that redirect
//! to a login page.
//!
//! Run with: cargo run --bin extractor-http-response --no-default-features
//! Test with:
//!   # Without header (should return plain HTTP 401)
//!   curl -v -X POST http://localhost:3000/hello.HelloWorldService/SayHello \
//!     -H 'Content-Type: application/json' \
//!     -d '{"name": "Alice"}'
//!
//!   # With header (should succeed)
//!   curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \
//!     -H 'Content-Type: application/json' \
//!     -H 'x-user-id: user123' \
//!     -d '{"name": "Alice"}'

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
// SocketAddr now provided by server_addr()

/// Custom rejection type that returns a plain HTTP 401 response.
/// This bypasses Connect protocol encoding entirely.
pub struct UnauthorizedResponse;

impl IntoResponse for UnauthorizedResponse {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Custom realm=\"app\"")],
            "Unauthorized: missing x-user-id header",
        )
            .into_response()
    }
}

/// Custom extractor that validates the x-user-id header.
/// Returns plain HTTP response on rejection (not ConnectError).
pub struct UserId(pub String);

impl<S> FromRequestParts<S> for UserId
where
    S: Send + Sync,
{
    type Rejection = UnauthorizedResponse;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| UserId(s.to_string()))
            .ok_or(UnauthorizedResponse)
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
    // Enable tracing to see the warning from handle_extractor_rejection
    tracing_subscriber::fmt::init();

    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build_connect();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example: Extractor Rejection with Plain HTTP Response ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("This example demonstrates handle_extractor_rejection (handler.rs:25)");
    println!("When extractor returns non-ConnectError, it's returned as-is (plain HTTP).");
    println!("Note: A warning is logged when this happens.");
    println!();
    println!("Test WITHOUT x-user-id header (should return plain HTTP 401):");
    println!("  curl -v -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
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
