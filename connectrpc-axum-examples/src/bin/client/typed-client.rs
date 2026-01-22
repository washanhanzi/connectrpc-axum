//! Typed Connect RPC Client Example
//!
//! Demonstrates using the generated typed client for cleaner, more ergonomic API calls.
//!
//! Instead of:
//! ```ignore
//! let response = client.call_unary("hello.HelloWorldService/SayHello", &request).await?;
//! ```
//!
//! You can write:
//! ```ignore
//! let response = client.say_hello(&request).await?;
//! ```
//!
//! Usage:
//!   # First, start a server in another terminal:
//!   cargo run --bin connect-unary --no-default-features
//!
//!   # Then run this client (defaults to http://localhost:3000):
//!   cargo run --bin typed-client --no-default-features
//!
//!   # Or specify a custom server URL:
//!   cargo run --bin typed-client --no-default-features -- http://localhost:8080

use connectrpc_axum_examples::{
    HelloRequest, HelloResponse, HelloWorldServiceClient,
    HELLO_WORLD_SERVICE_SERVICE_NAME, hello_world_service_procedures,
};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    println!("=== Typed Connect RPC Client Example ===");
    println!("Server URL: {}", base_url);
    println!("Service Name: {}", HELLO_WORLD_SERVICE_SERVICE_NAME);
    println!();

    // Create a typed client with default settings (JSON encoding)
    let client = HelloWorldServiceClient::new(&base_url)?;

    // Test 1: Basic typed call
    println!("Test 1: Typed say_hello() call...");
    {
        let request = HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec!["reading".to_string(), "coding".to_string()],
            greeting_type: None,
        };

        let response = client.say_hello(&request).await?;

        assert_eq!(response.message, "Hello, Alice!");
        println!("  PASS: Response message = {:?}", response.message);
        println!(
            "  PASS: Response has {} metadata headers",
            response.metadata().headers().len()
        );
    }

    // Test 2: Using the builder for custom configuration
    println!("Test 2: Typed client with proto encoding...");
    {
        let proto_client = HelloWorldServiceClient::builder(&base_url)
            .use_proto()
            .build()?;

        let request = HelloRequest {
            name: Some("Bob".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = proto_client.say_hello(&request).await?;

        assert_eq!(response.message, "Hello, Bob!");
        println!("  PASS: Response message = {:?}", response.message);
    }

    // Test 3: Using get_greeting (idempotent endpoint)
    println!("Test 3: Typed get_greeting() call...");
    {
        let request = HelloRequest {
            name: Some("Charlie".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client.get_greeting(&request).await?;

        // The server should respond with the same greeting logic
        println!("  PASS: Response message = {:?}", response.message);
    }

    // Test 4: Using procedure constants for reference
    println!("Test 4: Procedure path constants...");
    {
        println!("  SAY_HELLO path: {}", hello_world_service_procedures::SAY_HELLO);
        println!("  SAY_HELLO_STREAM path: {}", hello_world_service_procedures::SAY_HELLO_STREAM);
        println!("  GET_GREETING path: {}", hello_world_service_procedures::GET_GREETING);
    }

    // Test 5: Accessing the underlying ConnectClient
    println!("Test 5: Accessing underlying ConnectClient...");
    {
        let inner = client.inner();
        println!("  Base URL: {}", inner.base_url());
        println!("  Is Proto: {}", inner.is_proto());
    }

    // Test 6: Response wrapper functionality
    println!("Test 6: Response wrapper methods...");
    {
        let request = HelloRequest {
            name: Some("Diana".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client.say_hello(&request).await?;

        // Deref access
        println!("  Message length: {}", response.message.len());

        // into_inner()
        let inner: HelloResponse = response.into_inner();
        assert_eq!(inner.message, "Hello, Diana!");
        println!("  PASS: into_inner() works - message = {:?}", inner.message);
    }

    // Test 7: Multiple calls with the same client (client is Clone)
    println!("Test 7: Multiple calls with cloned client...");
    {
        let client2 = client.clone();

        let request = HelloRequest {
            name: Some("Eve".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client2.say_hello(&request).await?;
        assert_eq!(response.message, "Hello, Eve!");
        println!("  PASS: Cloned client works - message = {:?}", response.message);
    }

    println!();
    println!("=== All tests passed! ===");

    Ok(())
}
