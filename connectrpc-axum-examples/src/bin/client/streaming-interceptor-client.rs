//! Streaming Interceptor Integration Test
//!
//! Tests interceptors on all streaming types (server, client, bidi) for Issue #28 and #29.
//!
//! This test verifies that RPC-level interceptors work correctly with:
//! - Server streaming (client.call_server_stream with interceptors)
//! - Client streaming (client.call_client_stream with interceptors)
//! - Bidirectional streaming (client.call_bidi_stream with interceptors)
//!
//! The test starts its own embedded server that handles all three streaming types
//! and verifies interceptor headers are properly sent.
//!
//! Usage:
//!   # Run the self-contained test:
//!   cargo run --bin streaming-interceptor-client --no-default-features

use axum::http::HeaderMap;
use connectrpc_axum::prelude::*;
use connectrpc_axum_client::{
    ClosureInterceptor, ConnectClient, ConnectResponse as ClientResponse, HeaderInterceptor,
    RequestContext,
};
use connectrpc_axum_examples::{
    EchoRequest, EchoResponse, HelloRequest, HelloResponse, echo_service_connect,
    hello_world_service_connect,
};
use futures::{Stream, StreamExt, stream};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

// Shared port counter to avoid conflicts
static PORT_COUNTER: AtomicU16 = AtomicU16::new(14000);

fn get_next_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

// ============================================================================
// Server Handlers
// ============================================================================

/// Server streaming handler - returns multiple responses
/// Echoes the x-interceptor-header value in the response to verify it was received
async fn say_hello_stream(
    headers: HeaderMap,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    // Check for interceptor header
    let interceptor_value = headers
        .get("x-interceptor-header")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "NOT_FOUND".to_string());

    let response_stream = async_stream::stream! {
        // First message includes interceptor header verification
        yield Ok(HelloResponse {
            message: format!("Hello, {}! Interceptor header: {}", name, interceptor_value),
            response_type: None,
        });

        // Additional messages
        for i in 1..=2 {
            yield Ok(HelloResponse {
                message: format!("Stream message #{} for {}", i, name),
                response_type: None,
            });
        }

        // Final message
        yield Ok(HelloResponse {
            message: format!("Stream complete for {}", name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

/// Client streaming handler - collects all messages and responds once
/// Echoes the x-interceptor-header value in the response
async fn echo_client_stream(
    headers: HeaderMap,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut messages = Vec::new();

    // Check for interceptor header
    let interceptor_value = headers
        .get("x-interceptor-header")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "NOT_FOUND".to_string());

    // Consume all messages from the client stream
    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                messages.push(msg.message);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    // Respond with aggregated result including interceptor header
    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages [{}]. Interceptor: {}",
            messages.len(),
            messages.join(", "),
            interceptor_value
        ),
    }))
}

/// Bidirectional streaming handler - echoes each message
/// Includes the x-interceptor-header in the first response
async fn echo_bidi_stream(
    headers: HeaderMap,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let mut stream = streaming.into_stream();

    // Check for interceptor header
    let interceptor_value = headers
        .get("x-interceptor-header")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "NOT_FOUND".to_string());

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    // Create response stream that echoes each incoming message
    let response_stream = async_stream::stream! {
        let mut is_first = true;

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    let count = counter_clone.fetch_add(1, Ordering::SeqCst);

                    // Include interceptor header in first message
                    if is_first {
                        yield Ok(EchoResponse {
                            message: format!(
                                "Bidi #{}: {} [Interceptor: {}]",
                                count, msg.message, interceptor_value
                            ),
                        });
                        is_first = false;
                    } else {
                        yield Ok(EchoResponse {
                            message: format!("Bidi #{}: {}", count, msg.message),
                        });
                    }
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }

        // Send final message
        let count = counter_clone.fetch_add(1, Ordering::SeqCst);
        yield Ok(EchoResponse {
            message: format!("Bidi stream #{} complete", count),
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

// ============================================================================
// Test Server
// ============================================================================

async fn run_server(addr: SocketAddr) {
    // Build HelloWorldService router for server streaming
    let hello_router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    // Build EchoService router for client and bidi streaming
    let echo_router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .echo_bidi_stream(echo_bidi_stream)
        .build();

    // Combine routers with MakeServiceBuilder for HTTP/2 h2c support
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_router)
        .add_router(echo_router)
        .build();

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// Test Runner
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Streaming Interceptor Integration Test ===");
    println!();
    println!("This test verifies Issue #28 and #29:");
    println!("  - Interceptors work with server streaming");
    println!("  - Interceptors work with client streaming");
    println!("  - Interceptors work with bidirectional streaming");
    println!();

    let port = get_next_port();
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    let base_url = format!("http://{}", addr);

    // Start server in background
    tokio::spawn(run_server(addr));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut passed = 0;
    let mut failed = 0;

    // ========================================================================
    // Test 1: Server Streaming with HeaderInterceptor
    // ========================================================================
    println!("Test 1: Server streaming with HeaderInterceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "server-stream-test",
            ))
            .build()?;

        let request = HelloRequest {
            name: Some("StreamTest".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<_> = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = stream.next().await {
            let msg = result?;
            messages.push(msg.message);
        }

        // Verify interceptor header was received by server
        let first_msg = &messages[0];
        if first_msg.contains("server-stream-test") {
            println!("  PASS: Server received interceptor header");
            println!("  First message: {}", first_msg);
            passed += 1;
        } else {
            println!("  FAIL: Interceptor header not found in response");
            println!("  First message: {}", first_msg);
            failed += 1;
        }

        println!("  Received {} total messages", messages.len());
    }

    // ========================================================================
    // Test 2: Server Streaming with Closure Interceptor
    // ========================================================================
    println!("Test 2: Server streaming with closure Interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(ClosureInterceptor::new(|ctx: &mut RequestContext<'_>| {
                ctx.headers.insert(
                    "x-interceptor-header",
                    "closure-stream-test".parse().unwrap(),
                );
                Ok(())
            }))
            .build()?;

        let request = HelloRequest {
            name: Some("ClosureTest".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<_> = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let first_msg = stream.next().await.ok_or(anyhow::anyhow!("No messages"))??;

        if first_msg.message.contains("closure-stream-test") {
            println!("  PASS: Closure interceptor header received");
            passed += 1;
        } else {
            println!("  FAIL: Closure interceptor header not found");
            failed += 1;
        }
    }

    // ========================================================================
    // Test 3: Server Streaming with Chained Interceptors
    // ========================================================================
    println!("Test 3: Server streaming with chained interceptors...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_interceptor(HeaderInterceptor::new("x-first-header", "first"))
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "chained-stream-test",
            ))
            .with_interceptor(HeaderInterceptor::new("x-third-header", "third"))
            .build()?;

        let request = HelloRequest {
            name: Some("ChainedTest".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<_> = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let first_msg = stream.next().await.ok_or(anyhow::anyhow!("No messages"))??;

        if first_msg.message.contains("chained-stream-test") {
            println!("  PASS: Chained interceptors work with server streaming");
            passed += 1;
        } else {
            println!("  FAIL: Chained interceptors not working");
            failed += 1;
        }
    }

    // ========================================================================
    // Test 4: Client Streaming with HeaderInterceptor
    // ========================================================================
    println!("Test 4: Client streaming with HeaderInterceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge() // Required for client streaming
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "client-stream-test",
            ))
            .build()?;

        let messages = vec![
            EchoRequest {
                message: "msg1".to_string(),
            },
            EchoRequest {
                message: "msg2".to_string(),
            },
            EchoRequest {
                message: "msg3".to_string(),
            },
        ];
        let request_stream = stream::iter(messages);

        let response: ClientResponse<EchoResponse> = client
            .call_client_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoClientStream",
                request_stream,
            )
            .await?;

        let msg = response.into_inner();
        if msg.message.contains("client-stream-test") {
            println!("  PASS: Server received interceptor header in client stream");
            println!("  Response: {}", msg.message);
            passed += 1;
        } else {
            println!("  FAIL: Interceptor header not found in client stream response");
            println!("  Response: {}", msg.message);
            failed += 1;
        }
    }

    // ========================================================================
    // Test 5: Client Streaming with Closure Interceptor
    // ========================================================================
    println!("Test 5: Client streaming with closure Interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_interceptor(ClosureInterceptor::new(|ctx: &mut RequestContext<'_>| {
                ctx.headers.insert(
                    "x-interceptor-header",
                    "closure-client-test".parse().unwrap(),
                );
                Ok(())
            }))
            .build()?;

        let messages = vec![EchoRequest {
            message: "test".to_string(),
        }];
        let request_stream = stream::iter(messages);

        let response: ClientResponse<EchoResponse> = client
            .call_client_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoClientStream",
                request_stream,
            )
            .await?;

        let msg = response.into_inner();
        if msg.message.contains("closure-client-test") {
            println!("  PASS: Closure interceptor works with client streaming");
            passed += 1;
        } else {
            println!("  FAIL: Closure interceptor not working with client streaming");
            failed += 1;
        }
    }

    // ========================================================================
    // Test 6: Bidirectional Streaming with HeaderInterceptor
    // ========================================================================
    println!("Test 6: Bidi streaming with HeaderInterceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge() // Required for bidi streaming
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "bidi-stream-test",
            ))
            .build()?;

        let messages = vec![
            EchoRequest {
                message: "bidi1".to_string(),
            },
            EchoRequest {
                message: "bidi2".to_string(),
            },
        ];
        let request_stream = stream::iter(messages);

        let response: ClientResponse<_> = client
            .call_bidi_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoBidiStream",
                request_stream,
            )
            .await?;

        let mut stream = response.into_inner();
        let mut received_messages = Vec::new();

        while let Some(result) = stream.next().await {
            let msg = result?;
            received_messages.push(msg.message);
        }

        // First message should contain the interceptor header
        let first_msg = &received_messages[0];
        if first_msg.contains("bidi-stream-test") {
            println!("  PASS: Server received interceptor header in bidi stream");
            println!("  First message: {}", first_msg);
            passed += 1;
        } else {
            println!("  FAIL: Interceptor header not found in bidi stream response");
            println!("  First message: {}", first_msg);
            failed += 1;
        }

        println!("  Received {} bidi messages", received_messages.len());
    }

    // ========================================================================
    // Test 7: Bidi Streaming with Closure Interceptor
    // ========================================================================
    println!("Test 7: Bidi streaming with closure Interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_interceptor(ClosureInterceptor::new(|ctx: &mut RequestContext<'_>| {
                ctx.headers.insert(
                    "x-interceptor-header",
                    "closure-bidi-test".parse().unwrap(),
                );
                Ok(())
            }))
            .build()?;

        let messages = vec![EchoRequest {
            message: "test".to_string(),
        }];
        let request_stream = stream::iter(messages);

        let response: ClientResponse<_> = client
            .call_bidi_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoBidiStream",
                request_stream,
            )
            .await?;

        let mut stream = response.into_inner();
        let first_msg = stream.next().await.ok_or(anyhow::anyhow!("No messages"))??;

        if first_msg.message.contains("closure-bidi-test") {
            println!("  PASS: Closure interceptor works with bidi streaming");
            passed += 1;
        } else {
            println!("  FAIL: Closure interceptor not working with bidi streaming");
            failed += 1;
        }
    }

    // ========================================================================
    // Test 8: Bidi Streaming with Chained Interceptors
    // ========================================================================
    println!("Test 8: Bidi streaming with chained interceptors...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_interceptor(HeaderInterceptor::new("x-auth", "bearer-token"))
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "chained-bidi-test",
            ))
            .with_interceptor(HeaderInterceptor::new("x-request-id", "req-123"))
            .build()?;

        let messages = vec![EchoRequest {
            message: "chain".to_string(),
        }];
        let request_stream = stream::iter(messages);

        let response: ClientResponse<_> = client
            .call_bidi_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoBidiStream",
                request_stream,
            )
            .await?;

        let mut stream = response.into_inner();
        let first_msg = stream.next().await.ok_or(anyhow::anyhow!("No messages"))??;

        if first_msg.message.contains("chained-bidi-test") {
            println!("  PASS: Chained interceptors work with bidi streaming");
            passed += 1;
        } else {
            println!("  FAIL: Chained interceptors not working with bidi streaming");
            failed += 1;
        }
    }

    // ========================================================================
    // Test 9: Proto encoding with interceptors on server streaming
    // ========================================================================
    println!("Test 9: Proto encoding with interceptor on server streaming...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_proto()
            .with_interceptor(HeaderInterceptor::new(
                "x-interceptor-header",
                "proto-stream-test",
            ))
            .build()?;

        let request = HelloRequest {
            name: Some("ProtoStream".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response: ClientResponse<_> = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let first_msg = stream.next().await.ok_or(anyhow::anyhow!("No messages"))??;

        if first_msg.message.contains("proto-stream-test") {
            println!("  PASS: Proto encoding with interceptor works");
            passed += 1;
        } else {
            println!("  FAIL: Proto encoding with interceptor not working");
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
        println!("SUCCESS: All streaming interceptor tests passed!");
        println!();
        println!("Issue #28 (Middleware/Interceptor compatibility with streaming): RESOLVED");
        println!("  - HeaderInterceptor works with server streaming");
        println!("  - HeaderInterceptor works with client streaming");
        println!("  - HeaderInterceptor works with bidi streaming");
        println!();
        println!("Issue #29 (RPC-level interceptors): RESOLVED for header-level");
        println!("  - Closure interceptors work with all streaming types");
        println!("  - Chained interceptors work with all streaming types");
        println!("  - Interceptors work with both JSON and Proto encoding");
        println!();
        println!("Note: Message-level interception (like connect-go's WrapStreamingClient)");
        println!("is not yet implemented. Current interceptors operate at the request/response");
        println!("header level, not per-message level.");
        Ok(())
    } else {
        Err(anyhow::anyhow!("{} tests failed", failed))
    }
}
