//! Error Details Example
//!
//! This example demonstrates returning error details in ConnectRPC errors.
//! When name == "error", returns a ResourceExhausted error with a
//! google.rpc.RetryInfo detail that Go clients can decode.
//!
//! Run with: cargo run --bin error-details
//! Test with: go test -v -run TestErrorDetails

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use prost::Message;
use std::net::SocketAddr;

/// Handler that returns error details when name == "error"
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.as_deref().unwrap_or("World");

    if name == "error" {
        // Return an error with google.rpc.RetryInfo detail
        let retry_info_bytes = encode_retry_info(5); // 5 seconds retry delay

        return Err(
            ConnectError::new(Code::ResourceExhausted, "rate limited, please retry")
                .add_detail("google.rpc.RetryInfo", retry_info_bytes),
        );
    }

    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}

/// Encode a google.rpc.RetryInfo protobuf message manually.
///
/// RetryInfo has a single field:
///   google.protobuf.Duration retry_delay = 1;
///
/// We encode it by first encoding the Duration, then wrapping it in field 1.
fn encode_retry_info(seconds: i64) -> Vec<u8> {
    // Encode the Duration message (field 1 = seconds, field 2 = nanos)
    let duration = prost_types::Duration { seconds, nanos: 0 };
    let mut duration_bytes = Vec::new();
    duration.encode(&mut duration_bytes).unwrap();

    // Wrap in RetryInfo's field 1 (wire type 2 = length-delimited)
    let mut bytes = Vec::new();
    bytes.push(0x0a); // field 1, wire type 2 (length-delimited)
    bytes.push(duration_bytes.len() as u8);
    bytes.extend(duration_bytes);
    bytes
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build_connect();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Error Details Example ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Test with name='error' to get error details:");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"error\"}}'");

    axum::serve(listener, router).await?;
    Ok(())
}
