//! EndStream Metadata Test Server
//!
//! This example tests that EndStream frames include metadata correctly:
//! - Error metadata is merged into the EndStream frame
//! - Protocol headers (connect-*, grpc-*, content-type, etc.) are filtered
//! - Custom headers are preserved
//!
//! Run with: cargo run --bin endstream-metadata
//! Test with: cd go-client && go test -v -run TestEndStreamMetadata

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use futures::stream::BoxStream;
// SocketAddr now provided by server_addr()

/// Streaming handler that returns an error with custom metadata
///
/// The metadata should appear in the EndStream frame's "metadata" field
async fn say_hello_stream_with_metadata(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<BoxStream<'static, Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    use futures::StreamExt;

    let name = req.name.unwrap_or_default();

    // Case 1: Return error with custom metadata
    if name == "error-with-meta" {
        return Err(ConnectError::new(Code::Internal, "Error with metadata")
            .with_meta("x-error-id", "err-12345")
            .with_meta("x-request-id", "req-67890")
            .with_meta("x-custom-bin", "AAEC")); // Pre-encoded base64 for binary header
    }

    // Case 2: Return error with protocol headers (should be filtered)
    if name == "error-with-protocol-headers" {
        return Err(
            ConnectError::new(Code::InvalidArgument, "Error with protocol headers")
                .with_meta("x-custom", "should-appear")
                .with_meta("content-type", "should-be-filtered")
                .with_meta("grpc-status", "should-be-filtered")
                .with_meta("connect-timeout-ms", "should-be-filtered"),
        );
    }

    // Case 3: Normal stream that ends with error mid-stream (error has metadata)
    if name == "mid-stream-error" {
        let response_stream = async_stream::stream! {
            yield Ok(HelloResponse {
                message: "Message 1".to_string(),
                response_type: None,
            });
            yield Ok(HelloResponse {
                message: "Message 2".to_string(),
                response_type: None,
            });
            // Error mid-stream with metadata
            yield Err(ConnectError::new(Code::Aborted, "Stream aborted")
                .with_meta("x-abort-reason", "test-abort")
                .with_meta("x-message-count", "2"));
        };
        return Ok(ConnectResponse::new(StreamBody::new(
            response_stream.boxed(),
        )));
    }

    // Case 4: Normal successful stream (EndStream should have empty/no metadata)
    let response_stream = async_stream::stream! {
        for i in 1..=3 {
            yield Ok(HelloResponse {
                message: format!("Hello {}! Message {}", name, i),
                response_type: None,
            });
        }
    };

    Ok(ConnectResponse::new(StreamBody::new(
        response_stream.boxed(),
    )))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream_with_metadata)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== EndStream Metadata Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Test cases:");
    println!(
        "  1. name='error-with-meta'           -> Error with x-error-id, x-request-id metadata"
    );
    println!(
        "  2. name='error-with-protocol-headers' -> Protocol headers filtered, x-custom preserved"
    );
    println!("  3. name='mid-stream-error'          -> Messages then error with metadata");
    println!("  4. name='Alice'                     -> Normal stream, empty metadata");
    println!();
    println!("Run tests: cd go-client && go test -v -run TestEndStreamMetadata");

    axum::serve(listener, app).await?;
    Ok(())
}
