//! Bidi Streaming Integration Test
//!
//! Tests the bidirectional streaming RPC call against the Rust server.
//!
//! Demonstrates using the typed client API for bidi streaming:
//! ```ignore
//! let response = client.echo_bidi_stream(request_stream).await?;
//! ```
//!
//! Usage:
//!   # First, start the server in another terminal:
//!   cargo run --bin connect-bidi-stream --no-default-features
//!
//!   # Then run this test (defaults to http://localhost:3000):
//!   cargo run --bin bidi-stream-client --no-default-features
//!
//!   # Or specify a custom server URL:
//!   cargo run --bin bidi-stream-client --no-default-features -- http://localhost:8080

use connectrpc_axum_client::ClientError;
use connectrpc_axum_examples::{EchoRequest, EchoServiceClient, EchoServiceClientBuilder};
use futures::{stream, StreamExt};
use std::env;
use std::time::Duration;

/// Helper to build a client configured for bidi streaming.
///
/// For bidirectional streaming over plain HTTP (http://), we need to enable
/// HTTP/2 prior knowledge (h2c) since HTTP/1.1 doesn't support full-duplex
/// communication. For HTTPS URLs, HTTP/2 is negotiated via ALPN automatically.
fn bidi_client_builder(base_url: &str) -> EchoServiceClientBuilder {
    let builder = EchoServiceClient::builder(base_url);

    // Enable HTTP/2 prior knowledge for http:// URLs (required for bidi streaming)
    // For https:// URLs, HTTP/2 is negotiated via ALPN
    if base_url.starts_with("http://") {
        builder.http2_prior_knowledge()
    } else {
        builder
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check command line args first, then SERVER_URL env var, then default
    let base_url = env::args()
        .nth(1)
        .or_else(|| env::var("SERVER_URL").ok())
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    println!("=== Bidi Streaming Integration Tests ===");
    println!("Server URL: {}", base_url);
    println!();

    // Test 1: Bidi streaming with JSON encoding
    println!("Test 1: Bidi streaming with JSON encoding...");
    {
        let client = bidi_client_builder(&base_url).use_json().build()?;

        let messages = vec![
            EchoRequest {
                message: "hello".to_string(),
            },
            EchoRequest {
                message: "world".to_string(),
            },
            EchoRequest {
                message: "bidi".to_string(),
            },
        ];

        let request_stream = stream::iter(messages);

        let response = client.echo_bidi_stream(request_stream).await?;

        let mut stream = response.into_inner();
        let mut message_count = 0;

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    message_count += 1;
                    println!("  Got message #{}: {}", message_count, msg.message);
                    assert!(
                        msg.message.contains("Bidi Echo") || msg.message.contains("completed"),
                        "Expected 'Bidi Echo' or 'completed' in message"
                    );
                }
                Err(e) => {
                    println!("  FAIL: Unexpected error: {:?}", e);
                    return Err(anyhow::anyhow!("Unexpected error"));
                }
            }
        }

        // Server should echo 3 messages + 1 completion message = 4 total
        assert!(
            message_count >= 3,
            "Expected at least 3 messages, got {}",
            message_count
        );
        println!("  PASS: Received {} messages", message_count);
    }

    // Test 2: Bidi streaming with Proto encoding
    println!("Test 2: Bidi streaming with Proto encoding...");
    {
        let client = bidi_client_builder(&base_url).use_proto().build()?;

        let messages = vec![
            EchoRequest {
                message: "proto1".to_string(),
            },
            EchoRequest {
                message: "proto2".to_string(),
            },
        ];

        let request_stream = stream::iter(messages);

        let response = client.echo_bidi_stream(request_stream).await?;

        let mut stream = response.into_inner();
        let messages: Vec<_> = stream
            .by_ref()
            .filter_map(|r| async { r.ok() })
            .collect()
            .await;

        assert!(
            messages.len() >= 2,
            "Expected at least 2 messages, got {}",
            messages.len()
        );
        println!(
            "  PASS: Received {} messages with proto encoding",
            messages.len()
        );
    }

    // Test 3: Bidi streaming with single message
    println!("Test 3: Bidi streaming with single message...");
    {
        let client = bidi_client_builder(&base_url).use_json().build()?;

        let messages = vec![EchoRequest {
            message: "single".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let response = client.echo_bidi_stream(request_stream).await?;

        let mut stream = response.into_inner();
        let mut count = 0;

        while let Some(result) = stream.next().await {
            if result.is_ok() {
                count += 1;
            }
        }

        assert!(count >= 1, "Expected at least 1 message, got {}", count);
        println!("  PASS: Received {} messages from single input", count);
    }

    // Test 4: Connection error handling
    println!("Test 4: Connection error handling...");
    {
        // Note: For connection error test, we still need http2_prior_knowledge
        // because the error happens at the transport level, not protocol negotiation
        let client = bidi_client_builder("http://127.0.0.1:1")
            .use_json()
            .build()?;

        let messages = vec![EchoRequest {
            message: "test".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let result: Result<_, ClientError> = client.echo_bidi_stream(request_stream).await;

        match result {
            Err(ClientError::Transport(_)) => {
                println!("  PASS: Got expected Transport error");
            }
            Err(other) => {
                println!(
                    "  FAIL: Expected Transport error, got different error: {:?}",
                    other
                );
                return Err(anyhow::anyhow!("Unexpected error type"));
            }
            Ok(_) => {
                println!("  FAIL: Expected Transport error, but call succeeded");
                return Err(anyhow::anyhow!("Expected error but got success"));
            }
        }
    }

    // Test 5: Collect all messages using StreamExt
    println!("Test 5: Collect all messages using StreamExt...");
    {
        let client = bidi_client_builder(&base_url).use_json().build()?;

        let messages = vec![
            EchoRequest {
                message: "a".to_string(),
            },
            EchoRequest {
                message: "b".to_string(),
            },
            EchoRequest {
                message: "c".to_string(),
            },
        ];

        let request_stream = stream::iter(messages);

        let response = client.echo_bidi_stream(request_stream).await?;

        let stream = response.into_inner();
        let all_results: Vec<_> = stream.collect().await;
        let successful: Vec<_> = all_results.into_iter().filter_map(|r| r.ok()).collect();

        assert!(
            successful.len() >= 3,
            "Expected at least 3 successful messages, got {}",
            successful.len()
        );
        println!("  PASS: Collected {} successful messages", successful.len());
    }

    // Test 6: is_finished() works correctly
    println!("Test 6: is_finished() works correctly...");
    {
        let client = bidi_client_builder(&base_url).use_json().build()?;

        let messages = vec![EchoRequest {
            message: "finish-test".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let response = client.echo_bidi_stream(request_stream).await?;

        let mut stream = response.into_inner();

        // Not finished yet
        assert!(!stream.is_finished(), "Stream should not be finished yet");

        // Consume all messages
        while stream.next().await.is_some() {}

        // Now should be finished
        assert!(
            stream.is_finished(),
            "Stream should be finished after consuming all messages"
        );
        println!("  PASS: is_finished() transitions correctly");
    }

    // Test 7: Trailers access after stream consumption
    println!("Test 7: Trailers access after stream consumption...");
    {
        let client = bidi_client_builder(&base_url).use_json().build()?;

        let messages = vec![EchoRequest {
            message: "trailers-test".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let response = client.echo_bidi_stream(request_stream).await?;

        let mut stream = response.into_inner();

        // Consume all messages
        while stream.next().await.is_some() {}

        // Trailers may or may not be present depending on server implementation
        // Just verify we can access the method without panicking
        let _trailers = stream.trailers();
        println!("  PASS: trailers() accessible after stream consumption");
    }

    // Test 8: Timeout configuration (Connect-Timeout-Ms header)
    println!("Test 8: Timeout configuration...");
    {
        // Create a client with a 30-second timeout
        let client = bidi_client_builder(&base_url)
            .use_json()
            .timeout(Duration::from_secs(30))
            .build()?;

        let messages = vec![EchoRequest {
            message: "timeout-test".to_string(),
        }];

        let request_stream = stream::iter(messages);

        // The Connect-Timeout-Ms header will be set to 30000
        let response = client.echo_bidi_stream(request_stream).await?;

        let mut stream = response.into_inner();
        let mut count = 0;

        while let Some(result) = stream.next().await {
            if result.is_ok() {
                count += 1;
            }
        }

        assert!(count >= 1, "Expected at least 1 message, got {}", count);
        println!(
            "  PASS: Request with timeout succeeded, received {} messages",
            count
        );
    }

    println!();
    println!("=== All tests passed! ===");

    Ok(())
}
