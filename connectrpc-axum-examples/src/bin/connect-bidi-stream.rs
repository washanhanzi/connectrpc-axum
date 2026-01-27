//! Example: Connect Protocol Bidirectional Streaming
//!
//! This example demonstrates Connect protocol bidirectional streaming:
//! - Client sends a stream of messages
//! - Server responds with a stream of messages
//! - Uses `MakeServiceBuilder` for automatic HTTP/2 h2c support
//! - Uses the generated `EchoServiceBuilder` with declarative API
//!
//! Run with: cargo run --bin connect-bidi-stream
//! Test with: ./client connect-bidi

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, echo_service_connect};
use futures::{Stream, StreamExt};
// SocketAddr now provided by server_addr()
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;

/// Bidirectional streaming handler - echoes each message as it arrives
///
/// Uses `ConnectRequest<Streaming<T>>` for input and `StreamBody<S>` for output.
async fn echo_bidi_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let mut stream = streaming.into_stream();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    // Create response stream that echoes each incoming message
    let response_stream = async_stream::stream! {
        let mut message_count = 0;

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    message_count += 1;
                    let count = counter_clone.fetch_add(1, Ordering::SeqCst);
                    println!("Received message #{}: {}", message_count, msg.message);

                    yield Ok(EchoResponse {
                        message: format!(
                            "Bidi Echo #{} (msg #{}): {}",
                            count, message_count, msg.message
                        ),
                    });
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }

        // Send final message when client stream completes
        let count = counter_clone.fetch_add(1, Ordering::SeqCst);
        println!("Client stream complete. Sending final message.");
        yield Ok(EchoResponse {
            message: format!(
                "Bidi stream #{} completed. Echoed {} messages.",
                count, message_count
            ),
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Use generated builder - works for ALL streaming types including bidi streaming
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_bidi_stream(echo_bidi_stream)
        .build();

    // Use MakeServiceBuilder for automatic HTTP/2 h2c support
    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = TcpListener::bind(addr).await?;

    println!("=== Connect Protocol Bidi Streaming ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: EchoService");
    println!("  - EchoBidiStream (bidi streaming): POST /echo.EchoService/EchoBidiStream");
    println!();
    println!("Test with:");
    println!("  ./client connect-bidi");

    axum::serve(listener, app).await?;

    Ok(())
}
