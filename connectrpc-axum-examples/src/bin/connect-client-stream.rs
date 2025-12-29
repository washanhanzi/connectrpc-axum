//! Example: Connect Protocol Client Streaming
//!
//! This example demonstrates Connect protocol client streaming:
//! - Client sends a stream of messages
//! - Server responds with a single message after consuming the stream
//! - Uses the new ConnectStreamingRequest extractor
//!
//! Run with: cargo run --bin connect-client-stream
//! Test with curl (sending framed messages)

use connectrpc_axum::prelude::*;
use connectrpc_axum::message::ConnectStreamingRequest;
use connectrpc_axum_examples::{EchoRequest, EchoResponse};
use futures::StreamExt;
use std::net::SocketAddr;

/// Client streaming handler - collects all messages and responds once
async fn echo_client_stream(
    req: ConnectStreamingRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = req.stream;
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
    // Manually wire up the client streaming endpoint
    // (generated code would do this automatically once updated)
    let router = axum::Router::new()
        .route(
            "/echo.EchoService/EchoClientStream",
            post_connect_client_stream(echo_client_stream),
        )
        .layer(ConnectLayer::new());

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
