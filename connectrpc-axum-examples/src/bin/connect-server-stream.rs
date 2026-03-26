//! Example 2: Pure ConnectRPC Server Streaming
//!
//! This example demonstrates ConnectRPC server streaming:
//! - Server streaming RPC (single request, multiple responses)
//! - Uses async_stream for ergonomic stream creation
//! - No gRPC support (pure Connect protocol)
//!
//! Run with: cargo run --bin connect-server-stream --no-default-features
//! Test with Go client: go run ./cmd/client --protocol connect server-stream

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
use futures::Stream;
// SocketAddr now provided by server_addr()

/// Server streaming handler - returns multiple responses
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    let hobbies = req.hobbies;

    let response_stream = async_stream::stream! {
        // First greeting
        yield Ok(HelloResponse {
            message: format!("Hello, {}! Starting stream...", name),
            response_type: None,
        });

        // Stream hobbies if provided
        if !hobbies.is_empty() {
            for (idx, hobby) in hobbies.iter().enumerate() {
                yield Ok(HelloResponse {
                    message: format!("Hobby #{}: {} - nice!", idx + 1, hobby),
                    response_type: None,
                });
            }
        } else {
            // Send sample messages
            for i in 1..=3 {
                yield Ok(HelloResponse {
                    message: format!("Stream message #{} for {}", i, name),
                    response_type: None,
                });
            }
        }

        // Final message
        yield Ok(HelloResponse {
            message: format!("Stream complete. Goodbye, {}!", name),
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

    // MakeServiceBuilder applies ConnectLayer for protocol detection
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example 2: Pure ConnectRPC Server Streaming ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHelloStream (server streaming): POST /hello.HelloWorldService/SayHelloStream");
    println!();
    println!("Test with a framed Connect request:");
    println!(
        "{}",
        connectrpc_axum_examples::connect_streaming_curl_command(
            "/hello.HelloWorldService/SayHelloStream",
            r#"{"name": "Alice", "hobbies": ["coding", "reading"]}"#,
        )
    );
    println!();
    println!("The response body is Connect-framed, so raw curl output includes frame bytes.");
    println!("For decoded output, run: cargo run --bin server-stream-client");

    axum::serve(listener, app).await?;
    Ok(())
}
