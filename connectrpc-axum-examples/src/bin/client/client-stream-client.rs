//! Client Streaming Integration Test
//!
//! Tests the client streaming RPC call against the Rust server.
//!
//! Demonstrates using the typed client API for client streaming:
//! ```ignore
//! let response = client.echo_client_stream(request_stream).await?;
//! ```
//!
//! Usage:
//!   # First, start the server in another terminal:
//!   cargo run --bin connect-client-stream --no-default-features
//!
//!   # Then run this test (defaults to http://localhost:3000):
//!   cargo run --bin client-stream-client --no-default-features
//!
//!   # Or specify a custom server URL:
//!   cargo run --bin client-stream-client --no-default-features -- http://localhost:8080

use connectrpc_axum_client::ClientError;
use connectrpc_axum_examples::{
    EchoRequest,
    echo_service_connect_client::EchoServiceClient,
};
use futures::stream;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check command line args first, then SERVER_URL env var, then default
    let base_url = env::args()
        .nth(1)
        .or_else(|| env::var("SERVER_URL").ok())
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    println!("=== Client Streaming Integration Tests ===");
    println!("Server URL: {}", base_url);
    println!();

    // Test 1: Client streaming with JSON encoding
    println!("Test 1: Client streaming with JSON encoding...");
    {
        let client = EchoServiceClient::builder(&base_url).use_json().build()?;

        let messages = vec![
            EchoRequest {
                message: "first".to_string(),
            },
            EchoRequest {
                message: "second".to_string(),
            },
            EchoRequest {
                message: "third".to_string(),
            },
        ];

        let request_stream = stream::iter(messages);

        let response = client.echo_client_stream(request_stream).await?;

        assert!(
            response.message.contains("3 messages"),
            "Expected '3 messages' in response, got: {}",
            response.message
        );
        assert!(
            response.message.contains("first"),
            "Expected 'first' in response"
        );
        assert!(
            response.message.contains("second"),
            "Expected 'second' in response"
        );
        assert!(
            response.message.contains("third"),
            "Expected 'third' in response"
        );
        println!("  PASS: Response = {:?}", response.message);
    }

    // Test 2: Client streaming with Proto encoding
    println!("Test 2: Client streaming with Proto encoding...");
    {
        let client = EchoServiceClient::builder(&base_url).use_proto().build()?;

        let messages = vec![
            EchoRequest {
                message: "hello".to_string(),
            },
            EchoRequest {
                message: "world".to_string(),
            },
        ];

        let request_stream = stream::iter(messages);

        let response = client.echo_client_stream(request_stream).await?;

        assert!(
            response.message.contains("2 messages"),
            "Expected '2 messages' in response, got: {}",
            response.message
        );
        println!("  PASS: Response = {:?}", response.message);
    }

    // Test 3: Client streaming with single message
    println!("Test 3: Client streaming with single message...");
    {
        let client = EchoServiceClient::builder(&base_url).build()?;

        let messages = vec![EchoRequest {
            message: "only one".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let response = client.echo_client_stream(request_stream).await?;

        assert!(
            response.message.contains("1 messages"),
            "Expected '1 messages' in response, got: {}",
            response.message
        );
        println!("  PASS: Response = {:?}", response.message);
    }

    // Test 4: Client streaming with empty stream
    println!("Test 4: Client streaming with empty stream...");
    {
        let client = EchoServiceClient::builder(&base_url).build()?;

        let messages: Vec<EchoRequest> = vec![];
        let request_stream = stream::iter(messages);

        let response = client.echo_client_stream(request_stream).await?;

        assert!(
            response.message.contains("0 messages"),
            "Expected '0 messages' in response, got: {}",
            response.message
        );
        println!("  PASS: Response = {:?}", response.message);
    }

    // Test 5: Response wrapper methods
    println!("Test 5: Response wrapper methods (into_parts)...");
    {
        let client = EchoServiceClient::builder(&base_url).build()?;

        let messages = vec![EchoRequest {
            message: "test".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let response = client.echo_client_stream(request_stream).await?;

        // Test into_parts
        let (inner, metadata) = response.into_parts();
        assert!(!inner.message.is_empty());
        println!("  PASS: inner.message = {:?}", inner.message);
        println!("  PASS: metadata headers = {}", metadata.headers().len());
    }

    // Test 6: Connection error handling
    println!("Test 6: Connection error handling...");
    {
        let client = EchoServiceClient::builder("http://127.0.0.1:1")
            .use_json()
            .build()?;

        let messages = vec![EchoRequest {
            message: "test".to_string(),
        }];

        let request_stream = stream::iter(messages);

        let result: Result<_, ClientError> = client.echo_client_stream(request_stream).await;

        match result {
            Err(ClientError::Transport(_)) => {
                println!("  PASS: Got expected Transport error");
            }
            other => {
                println!("  FAIL: Expected Transport error, got: {:?}", other);
                return Err(anyhow::anyhow!("Unexpected result"));
            }
        }
    }

    // Test 7: Multiple sequential client streaming calls
    println!("Test 7: Multiple sequential client streaming calls...");
    {
        let client = EchoServiceClient::builder(&base_url).build()?;

        for i in 1..=3 {
            let messages: Vec<EchoRequest> = (1..=i)
                .map(|n| EchoRequest {
                    message: format!("message{}", n),
                })
                .collect();

            let request_stream = stream::iter(messages);

            let response = client.echo_client_stream(request_stream).await?;

            assert!(
                response.message.contains(&format!("{} messages", i)),
                "Expected '{} messages' in response, got: {}",
                i,
                response.message
            );
        }
        println!("  PASS: 3 sequential client streaming calls succeeded");
    }

    println!();
    println!("=== All tests passed! ===");

    Ok(())
}
