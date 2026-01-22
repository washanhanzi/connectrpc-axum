//! ConnectRPC Client Test
//!
//! Tests the connectrpc-axum-client against the Rust server.
//!
//! Usage:
//!   # First, start a server in another terminal:
//!   cargo run --bin connect-unary --no-default-features
//!
//!   # Then run the client test (defaults to http://localhost:3000):
//!   cargo run --bin client-test --no-default-features
//!
//!   # Or specify a custom server URL:
//!   cargo run --bin client-test --no-default-features -- http://localhost:8080

use connectrpc_axum_client::{ConnectClient, ConnectResponse as ClientResponse};
use connectrpc_axum_examples::{HelloRequest, HelloResponse};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    println!("=== ConnectRPC Client Tests ===");
    println!("Server URL: {}", base_url);
    println!();

    // Test 1: JSON encoding success
    println!("Test 1: Unary call with JSON encoding...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec!["reading".to_string(), "coding".to_string()],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        assert_eq!(response.message, "Hello, Alice!");
        println!("  PASS: Response message = {:?}", response.message);
        println!(
            "  PASS: Response has {} metadata headers",
            response.metadata().headers().len()
        );
    }

    // Test 2: Proto encoding success
    println!("Test 2: Unary call with Proto encoding...");
    {
        let client = ConnectClient::builder(&base_url).use_proto().build()?;

        let request = HelloRequest {
            name: Some("Bob".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        assert_eq!(response.message, "Hello, Bob!");
        println!("  PASS: Response message = {:?}", response.message);
    }

    // Test 3: Default name (None)
    println!("Test 3: Unary call with default name...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: None,
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        assert_eq!(response.message, "Hello, World!");
        println!("  PASS: Response message = {:?}", response.message);
    }

    // Test 4: Response wrapper methods
    println!("Test 4: ConnectResponse wrapper methods...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Charlie".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        // Test Deref - access inner fields directly
        assert!(!response.message.is_empty());
        println!("  PASS: Deref works - message.len() = {}", response.message.len());

        // Test map - transform the inner value
        let mapped = response.map(|r| r.message.len());
        assert!(*mapped > 0);
        println!("  PASS: map() works - message length = {}", *mapped);
    }

    // Test 5: into_parts
    println!("Test 5: ConnectResponse::into_parts()...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        let request = HelloRequest {
            name: Some("Diana".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        let (inner, metadata) = response.into_parts();
        assert_eq!(inner.message, "Hello, Diana!");
        println!("  PASS: inner.message = {:?}", inner.message);
        println!("  PASS: metadata headers = {}", metadata.headers().len());
    }

    // Test 6: Multiple sequential calls
    println!("Test 6: Multiple sequential calls...");
    {
        let client = ConnectClient::builder(&base_url).use_json().build()?;

        for i in 1..=3 {
            let request = HelloRequest {
                name: Some(format!("User{}", i)),
                hobbies: vec![],
                greeting_type: None,
            };

            let response: ClientResponse<HelloResponse> = client
                .call_unary("hello.HelloWorldService/SayHello", &request)
                .await?;

            assert_eq!(response.message, format!("Hello, User{}!", i));
        }
        println!("  PASS: 3 sequential calls succeeded");
    }

    // Test 7: Connection error (to verify error handling)
    println!("Test 7: Connection error handling...");
    {
        use connectrpc_axum_client::ConnectError;

        let client = ConnectClient::builder("http://127.0.0.1:1")
            .use_json()
            .build()?;

        let request = HelloRequest {
            name: Some("test".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let result: Result<ClientResponse<HelloResponse>, ConnectError> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await;

        match result {
            Err(ConnectError::Transport(_)) => {
                println!("  PASS: Got expected Transport error");
            }
            other => {
                println!("  FAIL: Expected Transport error, got: {:?}", other);
                return Err(anyhow::anyhow!("Unexpected result"));
            }
        }
    }

    println!();
    println!("=== All tests passed! ===");

    Ok(())
}
