//! Example: Client Streaming with Compression
//!
//! This example tests per-message decompression in streaming requests:
//! - Client sends compressed frames with flag 0x01
//! - Server decompresses using Connect-Content-Encoding: gzip
//! - Server echoes back received messages to verify decompression worked
//! - Uses the generated `EchoServiceBuilder` with declarative API
//!
//! Run with: cargo run --bin client-streaming-compression
//! Test with Go client: go test -v -run TestClientStreamingCompression

use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, echo_service_connect};
use futures::StreamExt;
// SocketAddr now provided by server_addr()

/// Client streaming handler - collects messages and returns summary
///
/// Uses `ConnectRequest<Streaming<T>>` - the unified streaming input type.
async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut messages = Vec::new();
    let mut stream = streaming.into_stream();

    while let Some(result) = stream.next().await {
        match result {
            Ok(req) => {
                messages.push(req.message);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages: [{}]",
            messages.len(),
            messages.join(", ")
        ),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Use generated builder - works for ALL streaming types including client streaming
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    // Enable compression
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Client Streaming Compression Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: EchoService");
    println!("  - EchoClientStream: POST /echo.EchoService/EchoClientStream");
    println!();
    println!("Test with:");
    println!("  go test -v -run TestClientStreamingCompression");

    axum::serve(listener, app).await?;
    Ok(())
}
