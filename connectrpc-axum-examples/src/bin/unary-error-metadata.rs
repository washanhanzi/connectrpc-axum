//! Example: Unary Error with Metadata
//!
//! This example demonstrates returning errors with custom metadata headers
//! in unary RPC responses. The metadata is returned as HTTP response headers.
//!
//! Run with: cargo run --bin unary-error-metadata --no-default-features
//! Test with Go client: go test -v -run TestUnaryErrorMetadata

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{helloworldservice, HelloRequest, HelloResponse};
use std::net::SocketAddr;

/// Handler that returns errors with custom metadata based on input
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_default();

    match name.as_str() {
        // Return error with custom metadata headers
        "error-with-meta" => Err(ConnectError::new(Code::Internal, "intentional error")
            .with_meta("x-error-id", "err-12345")
            .with_meta("x-request-id", "req-67890")
            .with_meta("x-custom-bin", "AAEC")), // Pre-encoded base64 for binary header

        // Return error with protocol headers that should appear (they're in error metadata)
        "error-with-protocol-headers" => {
            Err(ConnectError::new(Code::InvalidArgument, "error with protocol headers")
                .with_meta("x-custom", "should-appear")
                .with_meta("content-type", "should-appear-too") // Not filtered for unary
                .with_meta("grpc-status", "should-appear-too")) // Not filtered for unary
        }

        // Return error with multiple values for same header
        "error-multi-value" => Err(ConnectError::new(Code::FailedPrecondition, "multi-value error")
            .with_meta("x-multi", "value1")
            .with_meta("x-multi", "value2")),

        // Return error without metadata
        "error-no-meta" => Err(ConnectError::new(Code::NotFound, "resource not found")),

        // Success case
        _ => Ok(ConnectResponse::new(HelloResponse {
            message: format!("Hello, {}!", name),
            response_type: None,
        })),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build_connect();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example: Unary Error with Metadata ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHello (unary): POST /hello.HelloWorldService/SayHello");
    println!();
    println!("Test cases:");
    println!("  name='error-with-meta'    -> Internal error with x-error-id, x-request-id headers");
    println!("  name='error-multi-value'  -> FailedPrecondition with multi-value x-multi header");
    println!("  name='error-no-meta'      -> NotFound error without custom metadata");
    println!("  name='Alice'              -> Success response");

    axum::serve(listener, router).await?;
    Ok(())
}
