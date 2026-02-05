//! Typed Interceptor Integration Test
//!
//! Tests the typed interceptor system for generated clients.
//! Uses an embedded server that handles all RPC types and echoes custom headers.
//!
//! Scenarios tested:
//! 1. Closure-based before interceptor on unary (validates request + sets header)
//! 2. Closure-based after interceptor on unary (modifies response body)
//! 3. Struct-based before interceptor with state (Arc<AtomicUsize> counter)
//! 4. on_receive interceptor on server stream (counts received messages)
//! 5. on_send interceptor on client stream (rejects empty messages, aborts stream)
//! 6. Header propagation: before interceptor sets "x-custom-header", server echoes it
//!
//! Usage:
//!   cargo run --bin typed-interceptor-client --no-default-features

use axum::http::HeaderMap;
use connectrpc_axum::prelude::*;
use connectrpc_axum_client::{
    ClientError, RequestContext, TypedMutInterceptor, response_interceptor, stream_interceptor,
};
use connectrpc_axum_examples::{
    EchoRequest, EchoResponse, HelloRequest, HelloResponse,
    echo_service_connect, hello_world_service_connect,
    echo_service_connect_client::EchoServiceClient,
    hello_world_service_connect_client::HelloWorldServiceClient,
};
use futures::{Stream, StreamExt, stream};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

// Shared port counter to avoid conflicts
static PORT_COUNTER: AtomicU16 = AtomicU16::new(15000);

fn get_next_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

// ============================================================================
// Server Handlers
// ============================================================================

/// Unary handler that echoes "x-custom-header" in the response message
async fn say_hello(
    headers: HeaderMap,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let custom_header = headers
        .get("x-custom-header")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("NOT_FOUND");

    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! Header: {}", name, custom_header),
        response_type: None,
    }))
}

/// Server streaming handler - returns multiple responses
async fn say_hello_stream(
    headers: HeaderMap,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    let custom_header = headers
        .get("x-custom-header")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "NOT_FOUND".to_string());

    let response_stream = async_stream::stream! {
        yield Ok(HelloResponse {
            message: format!("Stream 1 for {}. Header: {}", name, custom_header),
            response_type: None,
        });
        yield Ok(HelloResponse {
            message: format!("Stream 2 for {}", name),
            response_type: None,
        });
        yield Ok(HelloResponse {
            message: format!("Stream 3 for {}", name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

/// Unary echo handler that echoes "x-custom-header" in the response
async fn echo(
    headers: HeaderMap,
    ConnectRequest(req): ConnectRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let custom_header = headers
        .get("x-custom-header")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("NOT_FOUND");

    Ok(ConnectResponse::new(EchoResponse {
        message: format!("{}|header:{}", req.message, custom_header),
    }))
}

/// Client streaming handler - collects all messages
async fn echo_client_stream(
    _headers: HeaderMap,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut messages = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => messages.push(msg.message),
            Err(e) => return Err(e),
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!("Received {}: [{}]", messages.len(), messages.join(", ")),
    }))
}

/// Bidi streaming handler - echoes each message
async fn echo_bidi_stream(
    _headers: HeaderMap,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let mut stream = streaming.into_stream();

    let response_stream = async_stream::stream! {
        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    yield Ok(EchoResponse {
                        message: format!("Echo: {}", msg.message),
                    });
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

// ============================================================================
// Test Server
// ============================================================================

async fn run_server(addr: SocketAddr) {
    let hello_router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .say_hello_stream(say_hello_stream)
        .build();

    let echo_router = echo_service_connect::EchoServiceBuilder::new()
        .echo(echo)
        .echo_client_stream(echo_client_stream)
        .echo_bidi_stream(echo_bidi_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_router)
        .add_router(echo_router)
        .build();

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// Struct-based interceptor with state (for Test 3)
// ============================================================================

/// A stateful interceptor that counts how many times it has been invoked.
#[derive(Clone)]
struct CountingInterceptor {
    counter: Arc<AtomicUsize>,
}

impl CountingInterceptor {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        Self { counter }
    }
}

impl TypedMutInterceptor<HelloRequest> for CountingInterceptor {
    fn intercept(
        &self,
        ctx: &mut RequestContext<'_>,
        _body: &mut HelloRequest,
    ) -> Result<(), ClientError> {
        let count = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        ctx.headers.insert(
            "x-custom-header",
            format!("count-{}", count).parse().unwrap(),
        );
        Ok(())
    }
}

// ============================================================================
// Test Runner
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Typed Interceptor Integration Test ===");
    println!();

    let port = get_next_port();
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    let base_url = format!("http://{}", addr);

    // Start embedded server
    tokio::spawn(run_server(addr));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut passed = 0;
    let mut failed = 0;

    // ========================================================================
    // Test 1: Closure-based before interceptor on unary
    //   - Validates request (rejects empty name)
    //   - Sets "x-custom-header"
    // ========================================================================
    println!("Test 1: Closure-based before interceptor on unary...");
    {
        let client = HelloWorldServiceClient::builder(&base_url)
            .with_before_say_hello(|ctx: &mut RequestContext<'_>, req: &mut HelloRequest| {
                if req.name.as_deref() == Some("") {
                    return Err(ClientError::invalid_argument("name must not be empty"));
                }
                ctx.headers.insert(
                    "x-custom-header",
                    "before-interceptor-value".parse().unwrap(),
                );
                Ok(())
            })
            .build()?;

        // Test 1a: Valid request - should pass and server should receive the header
        let request = HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };
        let response = client.say_hello(&request).await?;

        if response.message.contains("before-interceptor-value") {
            println!("  PASS (1a): Before interceptor set header, server received it");
            passed += 1;
        } else {
            println!(
                "  FAIL (1a): Header not found in response: {}",
                response.message
            );
            failed += 1;
        }

        // Test 1b: Invalid request - should fail before making the RPC
        let invalid_request = HelloRequest {
            name: Some("".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };
        match client.say_hello(&invalid_request).await {
            Err(e) => {
                let err_msg = format!("{}", e);
                if err_msg.contains("name must not be empty") {
                    println!("  PASS (1b): Before interceptor rejected invalid request");
                    passed += 1;
                } else {
                    println!("  FAIL (1b): Wrong error: {}", err_msg);
                    failed += 1;
                }
            }
            Ok(resp) => {
                println!(
                    "  FAIL (1b): Expected error but got response: {}",
                    resp.message
                );
                failed += 1;
            }
        }
    }

    // ========================================================================
    // Test 2: Closure-based after interceptor on unary
    //   - Modifies the response body (appends to message)
    // ========================================================================
    println!("Test 2: Closure-based after interceptor on unary...");
    {
        let client = HelloWorldServiceClient::builder(&base_url)
            .with_after_say_hello(response_interceptor(|_ctx, resp: &mut HelloResponse| {
                resp.message = format!("{} [intercepted]", resp.message);
                Ok(())
            }))
            .build()?;

        let request = HelloRequest {
            name: Some("Bob".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };
        let response = client.say_hello(&request).await?;

        if response.message.contains("[intercepted]") && response.message.contains("Hello, Bob!") {
            println!("  PASS: After interceptor modified response body");
            passed += 1;
        } else {
            println!("  FAIL: Response not modified: {}", response.message);
            failed += 1;
        }
    }

    // ========================================================================
    // Test 3: Struct-based before interceptor with state
    //   - Uses Arc<AtomicUsize> counter
    //   - Each call should increment the counter
    // ========================================================================
    println!("Test 3: Struct-based before interceptor with state...");
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let client = HelloWorldServiceClient::builder(&base_url)
            .with_before_say_hello(CountingInterceptor::new(counter.clone()))
            .build()?;

        let request = HelloRequest {
            name: Some("Carol".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        // Make 3 calls
        let r1 = client.say_hello(&request).await?;
        let r2 = client.say_hello(&request).await?;
        let r3 = client.say_hello(&request).await?;

        let final_count = counter.load(Ordering::SeqCst);

        if final_count == 3
            && r1.message.contains("count-1")
            && r2.message.contains("count-2")
            && r3.message.contains("count-3")
        {
            println!(
                "  PASS: Struct interceptor counted 3 calls, headers verified"
            );
            passed += 1;
        } else {
            println!(
                "  FAIL: Counter={}, r1={}, r2={}, r3={}",
                final_count, r1.message, r2.message, r3.message
            );
            failed += 1;
        }
    }

    // ========================================================================
    // Test 4: on_receive interceptor on server stream (SayHelloStream)
    //   - Counts received messages via shared counter
    // ========================================================================
    println!("Test 4: on_receive interceptor on server stream...");
    {
        let receive_counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = receive_counter.clone();

        let client = HelloWorldServiceClient::builder(&base_url)
            .with_on_receive_say_hello_stream(stream_interceptor(
                move |_ctx, _msg: &mut HelloResponse| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            ))
            .build()?;

        let request = HelloRequest {
            name: Some("Diana".to_string()),
            hobbies: vec![],
            greeting_type: None,
        };

        let response = client.say_hello_stream(&request).await?;
        let mut stream = response.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = stream.next().await {
            let msg = result?;
            messages.push(msg.message);
        }

        let count = receive_counter.load(Ordering::SeqCst);
        if count == 3 && messages.len() == 3 {
            println!(
                "  PASS: on_receive interceptor counted {} messages (expected 3)",
                count
            );
            passed += 1;
        } else {
            println!(
                "  FAIL: count={}, messages.len()={}, messages={:?}",
                count,
                messages.len(),
                messages
            );
            failed += 1;
        }
    }

    // ========================================================================
    // Test 5: on_send interceptor on client stream
    //   - Rejects messages with empty content (aborts the stream)
    // ========================================================================
    println!("Test 5: on_send interceptor on client stream rejects empty messages...");
    {
        let client = EchoServiceClient::builder(&base_url)
            .http2_prior_knowledge()
            .with_on_send_echo_client_stream(stream_interceptor(
                |_ctx, msg: &mut EchoRequest| {
                    if msg.message.is_empty() {
                        return Err(ClientError::invalid_argument("message must not be empty"));
                    }
                    Ok(())
                },
            ))
            .build()?;

        // Send: "hello", "", "world" — the empty message should abort the stream
        let messages = vec![
            EchoRequest {
                message: "hello".to_string(),
            },
            EchoRequest {
                message: "".to_string(),
            },
            EchoRequest {
                message: "world".to_string(),
            },
        ];
        let request_stream = stream::iter(messages);

        let result = client.echo_client_stream(request_stream).await;

        match result {
            Err(e) => {
                let err_msg = format!("{}", e);
                if err_msg.contains("message must not be empty") {
                    println!("  PASS: on_send interceptor aborted stream with correct error");
                    passed += 1;
                } else {
                    println!("  FAIL: Wrong error: {}", err_msg);
                    failed += 1;
                }
            }
            Ok(resp) => {
                // The server may receive only the first message before the abort
                let msg = resp.into_inner().message;
                if msg.contains("Received 1") {
                    // The stream was aborted after the first message, and the server
                    // only received "hello". The error was captured and returned.
                    // This means the interceptor error wasn't propagated back to caller.
                    // Check if the implementation stores the error.
                    println!(
                        "  FAIL: Expected error to be returned, but got response: {}",
                        msg
                    );
                    failed += 1;
                } else {
                    println!(
                        "  FAIL: Expected error but got success: {}",
                        msg
                    );
                    failed += 1;
                }
            }
        }
    }

    // ========================================================================
    // Test 6: Header propagation with before interceptor
    //   - Before interceptor sets "x-custom-header"
    //   - Server echoes it back in the response
    // ========================================================================
    println!("Test 6: Header propagation via before interceptor...");
    {
        let client = EchoServiceClient::builder(&base_url)
            .with_before_echo(|ctx: &mut RequestContext<'_>, _req: &mut EchoRequest| {
                ctx.headers.insert(
                    "x-custom-header",
                    "propagated-value".parse().unwrap(),
                );
                Ok(())
            })
            .build()?;

        let request = EchoRequest {
            message: "test".to_string(),
        };
        let response = client.echo(&request).await?;

        if response.message.contains("header:propagated-value") {
            println!("  PASS: Before interceptor header propagated to server");
            passed += 1;
        } else {
            println!(
                "  FAIL: Header not propagated. Response: {}",
                response.message
            );
            failed += 1;
        }
    }

    // ========================================================================
    // Summary
    // ========================================================================
    println!();
    println!("=== Typed Interceptor Test Results ===");
    println!("Passed: {}", passed);
    println!("Failed: {}", failed);
    println!();

    if failed == 0 {
        println!("=== All typed interceptor tests passed! ===");
        Ok(())
    } else {
        Err(anyhow::anyhow!("{} tests failed", failed))
    }
}
