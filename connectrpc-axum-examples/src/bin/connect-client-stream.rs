//! Example: Connect Protocol Client Streaming
//!
//! This example demonstrates Connect protocol client streaming:
//! - Client sends a stream of messages
//! - Server responds with a single message after consuming the stream
//! - Uses the generated `EchoServiceBuilder` with declarative API
//!
//! Run with: cargo run --bin connect-client-stream
//! Test with Go client: go run ./cmd/client --protocol connect client-stream

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, echoservice};
use futures::StreamExt;
use std::net::SocketAddr;

/// Client streaming handler - collects all messages and responds once
///
/// Uses `ConnectRequest<Streaming<T>>` - the unified streaming input type.
async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut messages = Vec::new();

    // Consume all messages from the client stream
    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                println!("Received message: {}", msg.message);
                messages.push(msg.message);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    println!("Client stream complete. Received {} messages.", messages.len());

    // Respond with aggregated result
    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Client Stream Complete: Received {} messages: [{}]",
            messages.len(),
            messages.join(", ")
        ),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Use generated builder - works for ALL streaming types including client streaming
    let router = echoservice::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build_connect();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Connect Protocol Client Streaming Example ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: EchoService");
    println!("  - EchoClientStream (client streaming): POST /echo.EchoService/EchoClientStream");
    println!();
    println!("Client streaming sends multiple framed messages:");
    println!("  Frame format: [flags:1][length:4][payload]");
    println!("  - flags=0x00 for message frames");
    println!("  - flags=0x02 for EndStream frame");
    println!();
    println!("Example test (requires a Connect client that supports streaming):");
    println!("  go run ./cmd/client --protocol connect client-stream");

    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
