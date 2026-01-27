//! Integration test server for receive_max_bytes feature.
//!
//! This server configures a small receive_max_bytes limit (1000 bytes) to test
//! that oversized requests return ResourceExhausted errors.
//!
//! Run with: cargo run --bin receive-max-bytes
//! Test with Go client: go test -v -run TestReceiveMaxBytes

use connectrpc_axum::MakeServiceBuilder;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, echo_service_connect};
use futures::StreamExt;
// SocketAddr now provided by server_addr()

/// Unary echo handler - just echoes back the message
async fn echo(
    ConnectRequest(req): ConnectRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Echo: {} ({} bytes)",
            req.message.chars().take(50).collect::<String>(),
            req.message.len()
        ),
    }))
}

/// Client streaming handler - collects all messages and responds once
async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut total_bytes = 0;
    let mut msg_count = 0;

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                total_bytes += msg.message.len();
                msg_count += 1;
                println!(
                    "Received message {}: {} bytes",
                    msg_count,
                    msg.message.len()
                );
            }
            Err(e) => {
                println!("Stream error: {:?}", e);
                return Err(e);
            }
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages, {} total bytes",
            msg_count, total_bytes
        ),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo(echo)
        .echo_client_stream(echo_client_stream)
        .build();

    // Configure with a small receive_max_bytes limit (1000 bytes)
    // This is intentionally small to easily test the limit
    let service = MakeServiceBuilder::new()
        .add_router(router)
        .receive_max_bytes(1000) // 1000 byte limit for requests
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== receive_max_bytes Integration Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Configuration:");
    println!("  - receive_max_bytes: 1000 bytes");
    println!();
    println!("Test scenarios:");
    println!("  - Small request (< 1000 bytes): should succeed");
    println!("  - Large request (> 1000 bytes): should return ResourceExhausted");
    println!("  - Stream with small messages: should succeed");
    println!("  - Stream with large message: should fail");

    axum::serve(listener, service).await?;
    Ok(())
}
