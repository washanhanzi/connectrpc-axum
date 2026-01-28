//! Message-Level Interceptor Integration Test (Issue #29)
//!
//! Tests per-message interception for streaming RPCs against the Go server.
//! This is the key feature from connect-go's WrapStreamingClient pattern.
//!
//! The test verifies that `MessageInterceptor` methods are called:
//! - `on_stream_send()` - Called for EVERY message sent to the server
//! - `on_stream_receive()` - Called for EVERY message received from the server
//!
//! This test runs against the Go reference server to ensure cross-implementation
//! compatibility.
//!
//! Usage:
//!   # Run against Go server (start Go server first):
//!   cd connectrpc-axum-examples/go-server && PORT=3000 go run .
//!   SERVER_URL=http://localhost:3000 cargo run --bin message-interceptor-client
//!
//!   # Or run against Rust server:
//!   cargo run --bin connect-bidi-stream
//!   SERVER_URL=http://localhost:3000 cargo run --bin message-interceptor-client

use connectrpc_axum_client::{
    ClientError, ConnectClient, ConnectResponse as ClientResponse, MessageInterceptor,
    RequestContext, ResponseContext, StreamContext, StreamType,
};
use connectrpc_axum_examples::{EchoRequest, EchoResponse, HelloRequest, HelloResponse};
use futures::{stream, StreamExt};
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use std::any::Any;
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Counting interceptor that tracks per-message interception.
///
/// This interceptor counts how many times each method is called,
/// allowing us to verify that streaming messages are intercepted.
#[derive(Clone)]
struct CountingMessageInterceptor {
    /// Count of `on_request` calls (unary)
    request_count: Arc<AtomicUsize>,
    /// Count of `on_response` calls (unary)
    response_count: Arc<AtomicUsize>,
    /// Count of `on_stream_send` calls (streaming outgoing)
    stream_send_count: Arc<AtomicUsize>,
    /// Count of `on_stream_receive` calls (streaming incoming)
    stream_receive_count: Arc<AtomicUsize>,
}

impl CountingMessageInterceptor {
    fn new() -> Self {
        Self {
            request_count: Arc::new(AtomicUsize::new(0)),
            response_count: Arc::new(AtomicUsize::new(0)),
            stream_send_count: Arc::new(AtomicUsize::new(0)),
            stream_receive_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn stream_send_count(&self) -> usize {
        self.stream_send_count.load(Ordering::SeqCst)
    }

    fn stream_receive_count(&self) -> usize {
        self.stream_receive_count.load(Ordering::SeqCst)
    }
}

impl MessageInterceptor for CountingMessageInterceptor {
    fn on_request<Req>(
        &self,
        ctx: &mut RequestContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        let count = self.request_count.fetch_add(1, Ordering::SeqCst);
        println!("  [on_request #{}] procedure={}", count + 1, ctx.procedure);
        Ok(())
    }

    fn on_response<Res>(
        &self,
        ctx: &ResponseContext,
        _response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        let count = self.response_count.fetch_add(1, Ordering::SeqCst);
        println!("  [on_response #{}] procedure={}", count + 1, ctx.procedure);
        Ok(())
    }

    fn on_stream_send<Req>(
        &self,
        ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        let count = self.stream_send_count.fetch_add(1, Ordering::SeqCst);

        // Try to get the message content for logging
        let msg_info = if let Some(echo_req) = (request as &dyn Any).downcast_ref::<EchoRequest>() {
            format!("message={:?}", echo_req.message)
        } else {
            format!("encoded_len={}", request.encoded_len())
        };

        println!(
            "  [on_stream_send #{}] procedure={}, stream_type={:?}, {}",
            count + 1,
            ctx.procedure,
            ctx.stream_type,
            msg_info
        );
        Ok(())
    }

    fn on_stream_receive<Res>(
        &self,
        ctx: &StreamContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        let count = self.stream_receive_count.fetch_add(1, Ordering::SeqCst);

        // Try to get the message content for logging
        let msg_info = if let Some(echo_res) = (response as &dyn Any).downcast_ref::<EchoResponse>()
        {
            format!("message={:?}", echo_res.message)
        } else if let Some(hello_res) =
            (response as &dyn Any).downcast_ref::<HelloResponse>()
        {
            format!("message={:?}", hello_res.message)
        } else {
            format!("encoded_len={}", response.encoded_len())
        };

        println!(
            "  [on_stream_receive #{}] procedure={}, stream_type={:?}, {}",
            count + 1,
            ctx.procedure,
            ctx.stream_type,
            msg_info
        );
        Ok(())
    }
}

/// Message-modifying interceptor that transforms messages.
///
/// This demonstrates the ability to modify messages in-flight,
/// which is one of the key use cases for message-level interception.
#[derive(Clone)]
struct ModifyingInterceptor {
    prefix: String,
}

impl ModifyingInterceptor {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
        }
    }
}

impl MessageInterceptor for ModifyingInterceptor {
    fn on_stream_send<Req>(
        &self,
        _ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        // Modify EchoRequest messages by prefixing the message
        if let Some(echo_req) = (request as &mut dyn Any).downcast_mut::<EchoRequest>() {
            echo_req.message = format!("{}{}", self.prefix, echo_req.message);
            println!(
                "  [ModifyingInterceptor] Modified outgoing: {}",
                echo_req.message
            );
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Get server URL from env or default
    let base_url = env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    println!("=== Message-Level Interceptor Integration Test (Issue #29) ===");
    println!("Server URL: {}", base_url);
    println!();
    println!("This test verifies per-message interception for streaming RPCs:");
    println!("  - on_stream_send() called for EACH outgoing message");
    println!("  - on_stream_receive() called for EACH incoming message");
    println!();

    let mut passed = 0;
    let mut failed = 0;

    // ========================================================================
    // Test 1: Server Streaming - Verify on_stream_receive() called per message
    // ========================================================================
    println!("Test 1: Server streaming - on_stream_receive() per message...");
    {
        let interceptor = CountingMessageInterceptor::new();
        let interceptor_clone = interceptor.clone();

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .with_message_interceptor(interceptor)
            .build()?;

        let request = HelloRequest {
            name: Some("StreamTest".to_string()),
            hobbies: vec!["reading".to_string(), "coding".to_string()],
            greeting_type: None,
        };

        let response: ClientResponse<_> = client
            .call_server_stream::<HelloRequest, HelloResponse>(
                "hello.HelloWorldService/SayHelloStream",
                &request,
            )
            .await?;

        let mut stream = response.into_inner();
        let mut message_count = 0;

        while let Some(result) = stream.next().await {
            let _msg = result?;
            message_count += 1;
        }

        let receive_count = interceptor_clone.stream_receive_count();

        println!("  Messages received: {}", message_count);
        println!("  on_stream_receive() calls: {}", receive_count);

        if receive_count == message_count && message_count > 0 {
            println!("  PASS: on_stream_receive() called for each message");
            passed += 1;
        } else if receive_count == 0 {
            println!("  FAIL: on_stream_receive() was NOT called (feature not implemented)");
            failed += 1;
        } else {
            println!(
                "  FAIL: Count mismatch - expected {}, got {}",
                message_count, receive_count
            );
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 2: Client Streaming - Verify on_stream_send() called per message
    // ========================================================================
    println!("Test 2: Client streaming - on_stream_send() per message...");
    {
        let interceptor = CountingMessageInterceptor::new();
        let interceptor_clone = interceptor.clone();

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_message_interceptor(interceptor)
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
        let message_count = messages.len();
        let request_stream = stream::iter(messages);

        let _response: ClientResponse<EchoResponse> = client
            .call_client_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoClientStream",
                request_stream,
            )
            .await?;

        let send_count = interceptor_clone.stream_send_count();

        println!("  Messages sent: {}", message_count);
        println!("  on_stream_send() calls: {}", send_count);

        if send_count == message_count {
            println!("  PASS: on_stream_send() called for each message");
            passed += 1;
        } else if send_count == 0 {
            println!("  FAIL: on_stream_send() was NOT called (feature not implemented)");
            failed += 1;
        } else {
            println!(
                "  FAIL: Count mismatch - expected {}, got {}",
                message_count, send_count
            );
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 3: Bidi Streaming - Both on_stream_send() and on_stream_receive()
    // ========================================================================
    println!("Test 3: Bidi streaming - both send and receive interception...");
    {
        let interceptor = CountingMessageInterceptor::new();
        let interceptor_clone = interceptor.clone();

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_message_interceptor(interceptor)
            .build()?;

        let messages = vec![
            EchoRequest {
                message: "bidi1".to_string(),
            },
            EchoRequest {
                message: "bidi2".to_string(),
            },
            EchoRequest {
                message: "bidi3".to_string(),
            },
        ];
        let sent_count = messages.len();
        let request_stream = stream::iter(messages);

        let response: ClientResponse<_> = client
            .call_bidi_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoBidiStream",
                request_stream,
            )
            .await?;

        let mut stream = response.into_inner();
        let mut received_count = 0;

        while let Some(result) = stream.next().await {
            let _msg = result?;
            received_count += 1;
        }

        let send_intercepted = interceptor_clone.stream_send_count();
        let receive_intercepted = interceptor_clone.stream_receive_count();

        println!("  Messages sent: {}", sent_count);
        println!("  Messages received: {}", received_count);
        println!("  on_stream_send() calls: {}", send_intercepted);
        println!("  on_stream_receive() calls: {}", receive_intercepted);

        let send_ok = send_intercepted == sent_count;
        let receive_ok = receive_intercepted == received_count && received_count > 0;

        if send_ok && receive_ok {
            println!("  PASS: Both send and receive intercepted correctly");
            passed += 1;
        } else if send_intercepted == 0 && receive_intercepted == 0 {
            println!("  FAIL: Neither send nor receive intercepted (feature not implemented)");
            failed += 1;
        } else {
            if !send_ok {
                println!(
                    "  FAIL: Send mismatch - expected {}, got {}",
                    sent_count, send_intercepted
                );
            }
            if !receive_ok {
                println!(
                    "  FAIL: Receive mismatch - expected {}, got {}",
                    received_count, receive_intercepted
                );
            }
            failed += 1;
        }
    }
    println!();

    // ========================================================================
    // Test 4: Message modification via interceptor
    // ========================================================================
    println!("Test 4: Message modification via interceptor...");
    {
        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_message_interceptor(ModifyingInterceptor::new("[MODIFIED] "))
            .build()?;

        let messages = vec![EchoRequest {
            message: "original".to_string(),
        }];
        let request_stream = stream::iter(messages);

        let response: ClientResponse<_> = client
            .call_bidi_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoBidiStream",
                request_stream,
            )
            .await?;

        let mut stream = response.into_inner();
        let first_response = stream.next().await;

        match first_response {
            Some(Ok(msg)) => {
                // The server should echo back the modified message
                if msg.message.contains("[MODIFIED]") {
                    println!("  PASS: Message was modified by interceptor");
                    println!("  Response: {}", msg.message);
                    passed += 1;
                } else {
                    println!("  FAIL: Message was NOT modified (feature not implemented)");
                    println!("  Response: {}", msg.message);
                    failed += 1;
                }
            }
            Some(Err(e)) => {
                println!("  FAIL: Error receiving response: {:?}", e);
                failed += 1;
            }
            None => {
                println!("  FAIL: No response received");
                failed += 1;
            }
        }
    }
    println!();

    // ========================================================================
    // Test 5: StreamContext provides correct stream type
    // ========================================================================
    println!("Test 5: StreamContext provides correct stream type...");
    {
        // Test that StreamContext.stream_type is correctly set

        #[derive(Clone)]
        struct StreamTypeChecker {
            expected_type: StreamType,
            type_matched: Arc<AtomicUsize>,
        }

        impl MessageInterceptor for StreamTypeChecker {
            fn on_stream_send<Req>(
                &self,
                ctx: &StreamContext,
                _request: &mut Req,
            ) -> Result<(), ClientError>
            where
                Req: Message + Serialize + 'static,
            {
                if ctx.stream_type == self.expected_type {
                    self.type_matched.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            }

            fn on_stream_receive<Res>(
                &self,
                ctx: &StreamContext,
                _response: &mut Res,
            ) -> Result<(), ClientError>
            where
                Res: Message + DeserializeOwned + Default + 'static,
            {
                if ctx.stream_type == self.expected_type {
                    self.type_matched.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            }
        }

        let checker = StreamTypeChecker {
            expected_type: StreamType::BidiStream,
            type_matched: Arc::new(AtomicUsize::new(0)),
        };
        let type_matched = checker.type_matched.clone();

        let client = ConnectClient::builder(&base_url)
            .use_json()
            .http2_prior_knowledge()
            .with_message_interceptor(checker)
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

        // Consume the stream
        let mut stream = response.into_inner();
        while stream.next().await.is_some() {}

        let matched = type_matched.load(Ordering::SeqCst);
        if matched > 0 {
            println!("  PASS: StreamContext.stream_type is correctly set");
            passed += 1;
        } else {
            println!(
                "  FAIL: StreamContext.stream_type check failed (interceptor not called or wrong type)"
            );
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
        println!("SUCCESS: All message-level interceptor tests passed!");
        println!();
        println!("Issue #29 (RPC-level interceptors) is FULLY RESOLVED:");
        println!("  - on_stream_send() called for each outgoing stream message");
        println!("  - on_stream_receive() called for each incoming stream message");
        println!("  - Message modification works in streaming RPCs");
        println!("  - StreamContext provides correct metadata");
        Ok(())
    } else {
        println!("FAILURE: {} tests failed", failed);
        println!();
        println!(
            "Issue #29 message-level interception for streaming is NOT fully implemented."
        );
        println!("The interceptor trait methods exist but are not called in streaming code paths.");
        Err(anyhow::anyhow!("{} tests failed", failed))
    }
}
