//! ConnectRPC Server Streaming Client Test
//!
//! Tests the connectrpc-axum-client server streaming against the Rust server.
//!
//! Usage:
//!   # First, start the server in another terminal:
//!   cargo run --bin connect-server-stream --no-default-features
//!
//!   # Then run the client test (defaults to http://localhost:3000):
//!   cargo run --bin server-stream-client --no-default-features
//!
//!   # Or specify a custom server URL:
//!   cargo run --bin server-stream-client --no-default-features -- http://localhost:8080

use connectrpc_axum_client::{Code, ConnectClient, ConnectError};
use connectrpc_axum_examples::{HelloRequest, HelloResponse};
use futures::StreamExt;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    println!("=== ConnectRPC Server Streaming Client Tests ===");
    println!("Server URL: {}", base_url);
    println!();

    // Test 1: Server stream with JSON encoding
    println!("Test 1: Server streaming with JSON encoding...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec!["reading".to_string(), "coding".to_string()],
            greeting_type: None,
        };

        let response = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        // Check initial response metadata
        println!("  Initial response has {} metadata headers", response.metadata().headers().len());

        let mut stream = response.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    println!("  Received: {}", msg.message);
                    messages.push(msg.message);
                }
                Err(e) => {
                    println!("  Error: {:?}", e);
                    return Err(anyhow::anyhow!("Unexpected error in stream"));
                }
            }
        }

        // Verify we got the expected messages
        assert!(messages.len() >= 3, "Expected at least 3 messages, got {}", messages.len());
        assert!(messages[0].contains("Starting stream"), "First message should mention starting");
        assert!(messages[1].contains("Hobby #1: reading"), "Should have first hobby");
        assert!(messages[2].contains("Hobby #2: coding"), "Should have second hobby");
        assert!(messages.last().unwrap().contains("Goodbye"), "Last message should say goodbye");

        println!("  PASS: Received {} messages", messages.len());

        // Check if trailers are available
        if let Some(trailers) = stream.trailers() {
            println!("  PASS: Trailers available ({} headers)", trailers.headers().len());
        } else {
            println!("  PASS: No trailers (expected for this server)");
        }
    }

    // Test 2: Server stream with Proto encoding
    println!("Test 2: Server streaming with Proto encoding...");
    {
        let client = ConnectClient::builder(&base_url).use_proto().build()?;

        let request = HelloRequest {
            name: Some("Bob".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let mut count = 0;

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    count += 1;
                    println!("  Message {}: {}", count, msg.message);
                }
                Err(e) => {
                    println!("  Error: {:?}", e);
                    return Err(anyhow::anyhow!("Unexpected error in stream"));
                }
            }
        }

        // Without hobbies, server sends: 1 greeting + 3 sample messages + 1 goodbye = 5
        assert!(count >= 4, "Expected at least 4 messages, got {}", count);
        println!("  PASS: Received {} messages", count);
    }

    // Test 3: Empty stream (no hobbies, but server still sends some messages)
    println!("Test 3: Server stream with default name...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: None,
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = stream.next().await {
            messages.push(result?);
        }

        // Should default to "World"
        assert!(messages[0].message.contains("World"), "Should use default name 'World'");
        println!("  PASS: Default name used, received {} messages", messages.len());
    }

    // Test 4: Connection error handling
    println!("Test 4: Connection error handling...");
    {
        let client = ConnectClient::builder("http://127.0.0.1:1")
            .use_json()
            .build()?;

        let request = HelloRequest {
            name: Some("test".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let result = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await;

        match result {
            Err(ConnectError::Transport(_)) => {
                println!("  PASS: Got expected Transport error");
            }
            Err(other) => {
                println!("  FAIL: Expected Transport error, got different error: {:?}", other);
                return Err(anyhow::anyhow!("Unexpected error type"));
            }
            Ok(_) => {
                println!("  FAIL: Expected Transport error, but call succeeded");
                return Err(anyhow::anyhow!("Expected error but got success"));
            }
        }
    }

    // Test 5: Collect all messages at once using collect
    println!("Test 5: Collect all messages with collect()...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Collector".to_string()),
            hobbies: vec!["test".to_string()],
            greeting_type: None,
        };

        let response = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let stream = response.into_inner();

        // Collect all results
        let results: Vec<Result<HelloResponse, ConnectError>> = stream.collect().await;
        let messages: Result<Vec<_>, _> = results.into_iter().collect();
        let messages = messages?;

        assert!(!messages.is_empty(), "Should receive at least one message");
        println!("  PASS: Collected {} messages", messages.len());
    }

    // Test 6: is_finished() check
    println!("Test 6: is_finished() works correctly...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Finisher".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();

        // Before consuming, not finished
        assert!(!stream.is_finished(), "Stream should not be finished before consuming");

        // Consume all messages
        while let Some(_) = stream.next().await {}

        // After consuming, should be finished
        assert!(stream.is_finished(), "Stream should be finished after consuming all messages");
        println!("  PASS: is_finished() returns correct values");
    }

    // Test 7: Verify trailers with custom server that sends them
    println!("Test 7: Trailers access after stream consumption...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Trailer".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();

        // Consume stream
        while let Some(result) = stream.next().await {
            let _ = result?;
        }

        // Trailers should be accessible now (even if empty/None)
        let trailers = stream.trailers();
        println!("  PASS: Trailers access works (trailers: {:?})", trailers.is_some());
    }

    println!();
    println!("=== All tests passed! ===");

    Ok(())
}
