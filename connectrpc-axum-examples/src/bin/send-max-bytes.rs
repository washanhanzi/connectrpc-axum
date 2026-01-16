//! Integration test server for send_max_bytes feature.
//!
//! This server configures a small send_max_bytes limit (100 bytes) to test
//! that oversized responses return ResourceExhausted errors.
//!
//! Run with: cargo run --bin send-max-bytes
//! Test with Go client: go test -v -run TestSendMaxBytes

use connectrpc_axum::prelude::*;
use connectrpc_axum::{MakeServiceBuilder, MessageLimits};
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use std::net::SocketAddr;

/// Handler that generates a response based on the request name:
/// - "small": Returns a small response (< 100 bytes)
/// - "large": Returns a large response (> 100 bytes) - should trigger ResourceExhausted
/// - anything else: Returns a normal greeting
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let message = match name.as_str() {
        "small" => "Hi".to_string(),
        "large" => {
            // Generate a response larger than 100 bytes
            "X".repeat(200)
        }
        _ => format!("Hello, {}!", name),
    };

    Ok(ConnectResponse::new(HelloResponse {
        message,
        response_type: None,
    }))
}

/// Streaming handler that generates messages of varying sizes
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<StreamBody<impl futures::Stream<Item = Result<HelloResponse, ConnectError>>>>, ConnectError>
{
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let stream = async_stream::stream! {
        match name.as_str() {
            "stream_large" => {
                // First message is small (should succeed)
                yield Ok(HelloResponse {
                    message: "First message".to_string(),
                    response_type: None,
                });
                // Second message is large (should trigger ResourceExhausted)
                yield Ok(HelloResponse {
                    message: "X".repeat(200),
                    response_type: None,
                });
                // This should not be sent
                yield Ok(HelloResponse {
                    message: "Third message".to_string(),
                    response_type: None,
                });
            }
            "stream_small" => {
                // All small messages
                for i in 1..=3 {
                    yield Ok(HelloResponse {
                        message: format!("Message {}", i),
                        response_type: None,
                    });
                }
            }
            _ => {
                yield Ok(HelloResponse {
                    message: format!("Hello, {}!", name),
                    response_type: None,
                });
            }
        }
    };

    Ok(ConnectResponse::new(StreamBody::new(stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build router without ConnectLayer (we'll add it via MakeServiceBuilder)
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .say_hello_stream(say_hello_stream)
        .build();

    // Configure with a small send_max_bytes limit (100 bytes)
    // This is intentionally small to easily test the limit
    let service = MakeServiceBuilder::new()
        .add_router(router)
        .message_limits(
            MessageLimits::default()
                .send_max_bytes(100) // 100 byte limit for responses
        )
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== send_max_bytes Integration Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Configuration:");
    println!("  - send_max_bytes: 100 bytes");
    println!();
    println!("Test scenarios:");
    println!("  - name='small': Response < 100 bytes (should succeed)");
    println!("  - name='large': Response > 100 bytes (should return ResourceExhausted)");
    println!("  - name='stream_small': All stream messages < 100 bytes (should succeed)");
    println!("  - name='stream_large': Second stream message > 100 bytes (should fail mid-stream)");

    axum::serve(listener, service).await?;
    Ok(())
}
