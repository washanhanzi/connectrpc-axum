//! Example: Client Streaming with Compression
//!
//! This example tests per-message decompression in streaming requests:
//! - Client sends compressed frames with flag 0x01
//! - Server decompresses using Connect-Content-Encoding: gzip
//! - Server echoes back received messages to verify decompression worked
//!
//! Run with: cargo run --bin client-streaming-compression
//! Test with Go client: go test -v -run TestClientStreamingCompression

use connectrpc_axum::CompressionConfig;
use connectrpc_axum::handler::post_client_stream;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse};
use futures::StreamExt;
use std::net::SocketAddr;

/// Client streaming handler - collects messages and returns summary
async fn echo_client_stream(
    ConnectStreamingRequest { stream }: ConnectStreamingRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut messages = Vec::new();
    let mut stream = stream;

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
    // Build manual route for client streaming
    let router = axum::Router::new().route(
        "/echo.EchoService/EchoClientStream",
        post_client_stream(echo_client_stream),
    );

    // Enable compression
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Client Streaming Compression Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: EchoService");
    println!("  - EchoClientStream: POST /echo.EchoService/EchoClientStream");
    println!();
    println!("Test with:");
    println!("  go test -v -run TestClientStreamingCompression");

    axum::serve(listener, tower::make::Shared::new(app)).await?;
    Ok(())
}
