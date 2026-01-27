//! Example: Streaming with Compression
//!
//! This example tests per-message compression in streaming responses:
//! - Uses CompressionConfig with low threshold (100 bytes)
//! - Sends messages large enough to trigger compression
//! - The Go client test verifies flag 0x01 is set on compressed frames
//!
//! Run with: cargo run --bin streaming-compression
//! Test with Go client: go test -v -run TestStreamingCompression

use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
use futures::Stream;
// SocketAddr now provided by server_addr()

/// Server streaming handler - returns messages that will be compressed
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let response_stream = async_stream::stream! {
        // Small message (won't be compressed - under 100 byte threshold)
        yield Ok(HelloResponse {
            message: format!("Hi {}!", name),
            response_type: None,
        });

        // Large message (will be compressed - over 100 byte threshold)
        // Create a message with repeated text to ensure it's compressible
        let large_text = format!(
            "Hello {name}! This is a much longer message that should definitely exceed the \
            100 byte compression threshold. We're including lots of repeated text to ensure \
            good compression: {} {} {} {}",
            "padding_text_for_compression ".repeat(5),
            "more_padding_text ".repeat(5),
            "even_more_text ".repeat(5),
            "final_padding ".repeat(5)
        );
        yield Ok(HelloResponse {
            message: large_text,
            response_type: None,
        });

        // Another large message
        yield Ok(HelloResponse {
            message: format!(
                "Stream message for {}: {} {} {}",
                name,
                "repeated_content_for_compression ".repeat(10),
                "more_repeated_content ".repeat(10),
                "final_content ".repeat(10)
            ),
            response_type: None,
        });

        // Final small message
        yield Ok(HelloResponse {
            message: format!("Bye {}!", name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    // Enable compression with low threshold (100 bytes)
    // Messages >= 100 bytes will be compressed when client sends Accept-Encoding: gzip
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Streaming Compression Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Compression: enabled (min_bytes=100)");
    println!("Service: HelloWorldService");
    println!("  - SayHelloStream: POST /hello.HelloWorldService/SayHelloStream");
    println!();
    println!("Test with:");
    println!("  go test -v -run TestStreamingCompression");

    axum::serve(listener, app).await?;
    Ok(())
}
