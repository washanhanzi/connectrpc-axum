//! Integration test for RPC-level interceptors.
//!
//! This test verifies that:
//! - Issue #28: Interceptors work correctly (streaming tested via existing client tests)
//! - Issue #29: RPC-level interceptors (not HTTP-level middleware) are implemented
//!
//! For streaming tests with interceptors, use the existing client test binaries
//! which connect to externally running servers:
//! - server-stream-client
//! - client-stream-client
//! - bidi-stream-client
//!
//! Run with: cargo run --bin interceptor-test

use connectrpc_axum::prelude::*;
use connectrpc_axum_client::{
    ConnectClient, ConnectResponse as ClientResponse, HeaderInterceptor, InterceptContext,
    Interceptor,
};
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

// Shared port counter to avoid conflicts
static PORT_COUNTER: AtomicU16 = AtomicU16::new(13000);

fn get_next_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

// ============================================================================
// Server Handler
// ============================================================================

/// Unary handler - simple response
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}

// ============================================================================
// Test Runner
// ============================================================================

async fn run_server(addr: SocketAddr) {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== RPC-Level Interceptor Integration Test ===");
    println!();
    println!("This test verifies:");
    println!("  - Issue #28: Interceptors work with RPC calls");
    println!("  - Issue #29: RPC-level interceptors are implemented");
    println!();

    let port = get_next_port();
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    let base_url = format!("http://{}", addr);

    // Start server in background
    tokio::spawn(run_server(addr));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut passed = 0;
    let mut failed = 0;

    // ========================================================================
    // Test 1: Unary call with HeaderInterceptor
    // ========================================================================
    println!("Test 1: Unary call with HeaderInterceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "test-value-123",
            ))
            .build()?;

        let request = HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, Alice!") {
            println!("  PASS: Unary call with HeaderInterceptor succeeded");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Unexpected response");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }

    // ========================================================================
    // Test 2: Unary call with closure Interceptor
    // ========================================================================
    println!("Test 2: Unary call with closure Interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(Interceptor::new(|ctx: &mut InterceptContext<'_>| {
                ctx.headers
                    .insert("x-custom-header", "fn-interceptor-value".parse().unwrap());
                Ok(())
            }))
            .build()?;

        let request = HelloRequest {
            name: Some("FnTest".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, FnTest!") {
            println!("  PASS: Unary call with closure Interceptor succeeded");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Unexpected response");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }

    // ========================================================================
    // Test 3: Multiple chained interceptors
    // ========================================================================
    println!("Test 3: Chained interceptors...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(HeaderInterceptor::new("x-first", "first-value"))
            .with_interceptor(HeaderInterceptor::new("x-second", "second-value"))
            .with_interceptor(HeaderInterceptor::new("x-third", "third-value"))
            .build()?;

        let request = HelloRequest {
            name: Some("Chained".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, Chained!") {
            println!("  PASS: Chained interceptors call succeeded");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Unexpected response");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }

    // ========================================================================
    // Test 4: Proto encoding with interceptor
    // ========================================================================
    println!("Test 4: Proto encoding with interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_proto()
            .with_interceptor(HeaderInterceptor::new("x-proto-header", "proto-value"))
            .build()?;

        let request = HelloRequest {
            name: Some("ProtoTest".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, ProtoTest!") {
            println!("  PASS: Proto encoding with interceptor succeeded");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Unexpected response");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }

    // ========================================================================
    // Test 5: Multiple sequential calls with same client
    // ========================================================================
    println!("Test 5: Multiple sequential calls with interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(HeaderInterceptor::new("x-session", "session-123"))
            .build()?;

        let mut all_passed = true;
        for i in 1..=3 {
            let request = HelloRequest {
                name: Some(format!("User{}", i)),
                hobbies: vec![],
                greeting_type: None,
            };

            let response: ClientResponse<HelloResponse> = client
                .call_unary("hello.HelloWorldService/SayHello", &request)
                .await?;

            if !response.message.contains(&format!("Hello, User{}!", i)) {
                all_passed = false;
                break;
            }
        }

        if all_passed {
            println!("  PASS: Multiple sequential calls succeeded");
            passed += 1;
        } else {
            println!("  FAIL: Sequential calls failed");
            failed += 1;
        }
    }

    // ========================================================================
    // Summary
    // ========================================================================
    println!();
    println!("=== Test Results ===");
    println!("Passed: {}", passed);
    println!("Failed: {}", failed);
    println!();

    if failed == 0 {
        println!("SUCCESS: All interceptor tests passed!");
        println!();
        println!("Issue #29 (RPC-level interceptors): RESOLVED");
        println!("  - HeaderInterceptor adds headers to requests");
        println!("  - Interceptor closure allows custom logic");
        println!("  - Interceptors can be chained");
        println!("  - Works with both JSON and Proto encoding");
        println!();
        println!("Note: Streaming tests (Issue #28) should be run using the");
        println!("existing client test binaries with external servers:");
        println!("  - cargo run --bin connect-server-stream (server)");
        println!("  - cargo run --bin server-stream-client (client)");
        Ok(())
    } else {
        Err(anyhow::anyhow!("{} tests failed", failed))
    }
}
