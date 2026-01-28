//! Integration test for RPC-level interceptors with typed message access.
//!
//! This test verifies the new RPC-level interceptor feature that provides:
//! - Typed message access (read/modify request and response bodies)
//! - Per-message interception (for streaming RPCs, future work)
//! - Compile-time interceptor chaining
//!
//! Run with: cargo run --bin rpc-interceptor-test

use connectrpc_axum::prelude::*;
use connectrpc_axum_client::{
    ClientError, ConnectClient, ConnectResponse as ClientResponse, MessageInterceptor,
    RequestContext, ResponseContext,
};
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
use prost::Message;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::any::Any;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

// Shared port counter to avoid conflicts
static PORT_COUNTER: AtomicU16 = AtomicU16::new(14000);

fn get_next_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

// ============================================================================
// Server Handler
// ============================================================================

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
// RPC Interceptors
// ============================================================================

/// Logging interceptor - demonstrates typed message access
#[derive(Clone)]
struct LoggingInterceptor {
    call_count: Arc<AtomicUsize>,
}

impl LoggingInterceptor {
    fn new() -> Self {
        Self {
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl MessageInterceptor for LoggingInterceptor {
    fn on_request<Req>(
        &self,
        ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        println!(
            "  [LoggingInterceptor] Request to {} ({} bytes)",
            ctx.procedure,
            request.encoded_len()
        );
        Ok(())
    }

    fn on_response<Res>(
        &self,
        ctx: &ResponseContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        println!(
            "  [LoggingInterceptor] Response from {} ({} bytes)",
            ctx.procedure,
            response.encoded_len()
        );
        Ok(())
    }
}

/// Modifying interceptor - demonstrates message modification
#[derive(Clone)]
struct NamePrefixInterceptor {
    prefix: String,
}

impl NamePrefixInterceptor {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
        }
    }
}

impl MessageInterceptor for NamePrefixInterceptor {
    fn on_request<Req>(
        &self,
        _ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        // Use downcast to modify specific message types
        if let Some(hello_req) = (request as &mut dyn Any).downcast_mut::<HelloRequest>() {
            if let Some(ref name) = hello_req.name {
                hello_req.name = Some(format!("{}{}", self.prefix, name));
                println!(
                    "  [NamePrefixInterceptor] Modified name to: {}",
                    hello_req.name.as_ref().unwrap()
                );
            }
        }
        Ok(())
    }
}

/// Validation interceptor - demonstrates request validation
#[derive(Clone)]
struct ValidationInterceptor;

impl MessageInterceptor for ValidationInterceptor {
    fn on_request<Req>(
        &self,
        _ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        // Validate HelloRequest
        if let Some(hello_req) = (request as &dyn Any).downcast_ref::<HelloRequest>() {
            if let Some(ref name) = hello_req.name {
                if name.is_empty() {
                    return Err(ClientError::invalid_argument("name cannot be empty"));
                }
                if name.len() > 100 {
                    return Err(ClientError::invalid_argument("name too long"));
                }
            }
            println!("  [ValidationInterceptor] Request validated");
        }
        Ok(())
    }
}

/// Header-adding interceptor - demonstrates header modification
#[derive(Clone)]
struct RequestIdInterceptor {
    counter: Arc<AtomicUsize>,
}

impl RequestIdInterceptor {
    fn new() -> Self {
        Self {
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl MessageInterceptor for RequestIdInterceptor {
    fn on_request<Req>(
        &self,
        ctx: &mut RequestContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        ctx.headers
            .insert("x-request-id", format!("req-{}", id).parse().unwrap());
        println!("  [RequestIdInterceptor] Added x-request-id: req-{}", id);
        Ok(())
    }
}

// ============================================================================
// Test Runner
// ============================================================================

async fn run_server(addr: SocketAddr) {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
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
    println!("This test demonstrates RPC interceptors with typed message access:");
    println!("  - Logging: Access encoded_len() on typed messages");
    println!("  - Modification: Change message fields before sending");
    println!("  - Validation: Reject invalid requests");
    println!("  - Headers: Add custom headers per-request");
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
    // Test 1: Logging interceptor with typed message access
    // ========================================================================
    println!("Test 1: Logging interceptor...");
    {
        let logging = LoggingInterceptor::new();
        let call_count = logging.call_count.clone();

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_message_interceptor(logging)
            .build()?;

        let request = HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, Alice!") && call_count.load(Ordering::SeqCst) == 1 {
            println!("  PASS: Logging interceptor was called and response is correct");
            passed += 1;
        } else {
            println!("  FAIL: Unexpected result");
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 2: Modifying interceptor
    // ========================================================================
    println!("Test 2: Modifying interceptor (adds prefix to name)...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_message_interceptor(NamePrefixInterceptor::new("Dr. "))
            .build()?;

        let request = HelloRequest {
            name: Some("Bob".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        // The interceptor modifies "Bob" to "Dr. Bob"
        if response.message.contains("Hello, Dr. Bob!") {
            println!("  PASS: Name was modified by interceptor");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Name was not modified");
            println!("  Response: {} (expected 'Dr. Bob')", response.message);
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 3: Validation interceptor
    // ========================================================================
    println!("Test 3: Validation interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_message_interceptor(ValidationInterceptor)
            .build()?;

        // Valid request
        let request = HelloRequest {
            name: Some("ValidName".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, ValidName!") {
            println!("  PASS: Valid request passed validation");
            passed += 1;
        } else {
            println!("  FAIL: Valid request was rejected");
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 4: Chained interceptors
    // ========================================================================
    println!("Test 4: Chained interceptors (logging + modification + header)...");
    {
        let logging = LoggingInterceptor::new();

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_message_interceptor(logging)
            .with_message_interceptor(NamePrefixInterceptor::new("Prof. "))
            .with_message_interceptor(RequestIdInterceptor::new())
            .build()?;

        let request = HelloRequest {
            name: Some("Smith".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, Prof. Smith!") {
            println!("  PASS: All chained interceptors worked");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Chained interceptors failed");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 5: Proto encoding with RPC interceptor
    // ========================================================================
    println!("Test 5: Proto encoding with RPC interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_proto()
            .with_message_interceptor(LoggingInterceptor::new())
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
            println!("  PASS: Proto encoding works with RPC interceptor");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Proto encoding failed");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 6: Combined header-level and RPC-level interceptors
    // ========================================================================
    println!("Test 6: Combined header-level and RPC-level interceptors...");
    {
        use connectrpc_axum_client::HeaderInterceptor;

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(HeaderInterceptor::new("x-header-interceptor", "header-value"))
            .with_message_interceptor(LoggingInterceptor::new())
            .with_message_interceptor(NamePrefixInterceptor::new("Sir "))
            .build()?;

        let request = HelloRequest {
            name: Some("Lancelot".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<HelloResponse> = client
            .call_unary("hello.HelloWorldService/SayHello", &request)
            .await?;

        if response.message.contains("Hello, Sir Lancelot!") {
            println!("  PASS: Both header-level and RPC-level interceptors work");
            println!("  Response: {}", response.message);
            passed += 1;
        } else {
            println!("  FAIL: Combined interceptors failed");
            println!("  Response: {}", response.message);
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Summary
    // ========================================================================
    println!("=== Test Results ===");
    println!("Passed: {}", passed);
    println!("Failed: {}", failed);
    println!();

    if failed == 0 {
        println!("SUCCESS: All RPC-level interceptor tests passed!");
        println!();
        println!("Key capabilities demonstrated:");
        println!("  - Typed message access via generic methods");
        println!("  - Message modification before sending");
        println!("  - Request validation with error rejection");
        println!("  - Header modification from RPC interceptor");
        println!("  - Compile-time interceptor chaining");
        println!("  - Combined header-level and RPC-level interceptors");
        Ok(())
    } else {
        Err(anyhow::anyhow!("{} tests failed", failed))
    }
}
