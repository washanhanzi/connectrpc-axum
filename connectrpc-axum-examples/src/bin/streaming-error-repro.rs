//! Reproduction: Streaming handler errors bypass Connect framing
//!
//! This example demonstrates the bug where a streaming handler that returns
//! `Err(ConnectError)` BEFORE the stream starts produces an incorrect response:
//!
//! - INCORRECT: HTTP 4xx/5xx with `application/json` (what currently happens)
//! - CORRECT: HTTP 200 with `application/connect+json` + EndStream error frame
//!
//! Run with: cargo run --bin streaming-error-repro
//! Test with Go client: cd go-client && go run ./cmd/client stream-error

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use futures::Stream;
use std::net::SocketAddr;

/// Streaming handler that returns an error BEFORE producing any stream
///
/// This simulates common scenarios like:
/// - Authentication/authorization failure
/// - Input validation error
/// - Resource not found
/// - Rate limiting
async fn say_hello_stream_with_error(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_default();

    // Simulate an authorization check that fails
    if name == "unauthorized" {
        return Err(ConnectError::new(
            Code::PermissionDenied,
            "You are not authorized to access this stream",
        ));
    }

    // Simulate input validation failure
    if name == "invalid" {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "Invalid name provided",
        ));
    }

    // Simulate resource not found
    if name == "notfound" {
        return Err(ConnectError::new(
            Code::NotFound,
            "Requested resource does not exist",
        ));
    }

    // Normal case: return a stream
    let response_stream = async_stream::stream! {
        yield Ok(HelloResponse {
            message: format!("Hello, {}! Stream starting...", name),
            response_type: None,
        });

        for i in 1..=3 {
            yield Ok(HelloResponse {
                message: format!("Message {} for {}", i, name),
                response_type: None,
            });
        }

        yield Ok(HelloResponse {
            message: format!("Goodbye, {}!", name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let connect_router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream_with_error)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Streaming Error Reproduction Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("This server demonstrates the streaming error handling bug.");
    println!();
    println!("Test cases (use Go client: go run ./cmd/client stream-error):");
    println!();
    println!("  1. name='unauthorized' -> should return PermissionDenied");
    println!("  2. name='invalid'      -> should return InvalidArgument");
    println!("  3. name='notfound'     -> should return NotFound");
    println!("  4. name='Alice'        -> should stream normally");
    println!();
    println!("BUG: Cases 1-3 currently return:");
    println!("  - HTTP 4xx with Content-Type: application/json");
    println!();
    println!("CORRECT behavior per Connect protocol:");
    println!("  - HTTP 200 with Content-Type: application/connect+json");
    println!("  - Error in EndStream frame (flag 0x02)");
    println!();
    println!("Manual test:");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHelloStream \\");
    println!("    -H 'Content-Type: application/connect+json' \\");
    println!("    -d '{{\"name\": \"unauthorized\"}}' -v");

    axum::serve(listener, connect_router.into_make_service()).await?;
    Ok(())
}
