//! Example: Streaming handlers with extractors and state
//!
//! This example demonstrates that streaming handlers (server, client, bidi)
//! can use axum extractors and state just like unary handlers.
//!
//! Run with: cargo run --bin streaming-extractor
//! Test with: go test -v -run TestStreamingExtractor

use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, HelloRequest, HelloResponse};
use futures::{Stream, StreamExt};
// SocketAddr now provided by server_addr()
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Shared application state
#[derive(Clone)]
struct AppState {
    request_counter: Arc<AtomicUsize>,
    greeting_prefix: String,
}

/// Extract user ID from headers - returns error if missing
fn extract_user_id(headers: &HeaderMap) -> Result<String, ConnectError> {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .ok_or_else(|| ConnectError::new(Code::Unauthenticated, "Missing x-user-id header"))
}

/// Server streaming handler with State and HeaderMap extractors
async fn say_hello_stream_with_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let user_id = extract_user_id(&headers)?;
    let request_num = state.request_counter.fetch_add(1, Ordering::SeqCst);
    let name = req.name.unwrap_or_else(|| "World".to_string());
    let prefix = state.greeting_prefix.clone();

    let response_stream = async_stream::stream! {
        // First message includes state info
        yield Ok(HelloResponse {
            message: format!(
                "{} {}! (user={}, request #{})",
                prefix, name, user_id, request_num
            ),
            response_type: None,
        });

        // Stream additional messages
        for i in 1..=3 {
            yield Ok(HelloResponse {
                message: format!("Stream message #{} for {} (user={})", i, name, user_id),
                response_type: None,
            });
        }

        // Final message
        yield Ok(HelloResponse {
            message: format!("Stream complete for {} (request #{})", name, request_num),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

/// Client streaming handler with State and HeaderMap extractors
async fn echo_client_stream_with_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let user_id = extract_user_id(&headers)?;
    let request_num = state.request_counter.fetch_add(1, Ordering::SeqCst);

    let mut stream = streaming.into_stream();
    let mut messages = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                println!(
                    "[request #{}] Received from {}: {}",
                    request_num, user_id, msg.message
                );
                messages.push(msg.message);
            }
            Err(e) => return Err(e),
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Client stream from {} (request #{}) received {} messages: [{}]",
            user_id,
            request_num,
            messages.len(),
            messages.join(", ")
        ),
    }))
}

/// Bidi streaming handler with State and HeaderMap extractors
async fn echo_bidi_stream_with_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let user_id = extract_user_id(&headers)?;
    let request_num = state.request_counter.fetch_add(1, Ordering::SeqCst);

    let mut stream = streaming.into_stream();

    let response_stream = async_stream::stream! {
        let mut message_count = 0;

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    message_count += 1;
                    println!(
                        "[request #{}] Bidi from {}: {}",
                        request_num, user_id, msg.message
                    );

                    yield Ok(EchoResponse {
                        message: format!(
                            "Bidi echo #{} for {} (request #{}): {}",
                            message_count, user_id, request_num, msg.message
                        ),
                    });
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }

        // Final message
        yield Ok(EchoResponse {
            message: format!(
                "Bidi stream complete for {} (request #{}). Echoed {} messages.",
                user_id, request_num, message_count
            ),
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = AppState {
        request_counter: Arc::new(AtomicUsize::new(0)),
        greeting_prefix: "Hello".to_string(),
    };

    // Build router using post_connect with extractors
    // This demonstrates that streaming handlers work with State and other extractors
    let router = Router::new()
        .route(
            "/hello.HelloWorldService/SayHelloStream",
            post_connect(say_hello_stream_with_state),
        )
        .route(
            "/echo.EchoService/EchoClientStream",
            post_connect(echo_client_stream_with_state),
        )
        .route(
            "/echo.EchoService/EchoBidiStream",
            post_connect(echo_bidi_stream_with_state),
        )
        .layer(ConnectLayer::new())
        .with_state(state);

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Streaming Handlers with Extractors Example ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("All streaming handlers use State<AppState> and HeaderMap extractors.");
    println!("Requests without x-user-id header will fail with UNAUTHENTICATED.");
    println!();
    println!("Services:");
    println!("  - SayHelloStream (server streaming): POST /hello.HelloWorldService/SayHelloStream");
    println!("  - EchoClientStream (client streaming): POST /echo.EchoService/EchoClientStream");
    println!("  - EchoBidiStream (bidi streaming): POST /echo.EchoService/EchoBidiStream");
    println!();
    println!("Test with: go test -v -run TestStreamingExtractor");

    axum::serve(listener, router).await?;
    Ok(())
}
