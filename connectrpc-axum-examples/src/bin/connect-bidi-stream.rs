//! Example: Connect Protocol Bidirectional Streaming with HTTP/2 (h2c)
//!
//! This example demonstrates Connect protocol bidirectional streaming:
//! - Client sends a stream of messages
//! - Server responds with a stream of messages
//! - Uses HTTP/2 cleartext (h2c) for full-duplex communication
//!
//! Run with: cargo run --bin connect-bidi-stream
//! Test with: ./client connect-bidi

use connectrpc_axum::prelude::*;
use connectrpc_axum::message::ConnectStreamingRequest;
use connectrpc_axum_examples::{EchoRequest, EchoResponse};
use futures::{Stream, StreamExt};
use hyper::server::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::service::TowerToHyperService;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;

/// Bidirectional streaming handler - echoes each message as it arrives
async fn echo_bidi_stream(
    req: ConnectStreamingRequest<EchoRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let mut stream = req.stream;
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
    // Build router with bidi streaming endpoint
    let router = axum::Router::new()
        .route(
            "/echo.EchoService/EchoBidiStream",
            post_bidi_stream(echo_bidi_stream),
        )
        .layer(ConnectLayer::new());

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = TcpListener::bind(addr).await?;

    println!("=== Connect Protocol Bidi Streaming (HTTP/2 h2c) ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: EchoService");
    println!("  - EchoBidiStream (bidi streaming): POST /echo.EchoService/EchoBidiStream");
    println!();
    println!("This server uses HTTP/2 cleartext (h2c) for full-duplex streaming.");
    println!();
    println!("Test with:");
    println!("  ./client connect-bidi");

    // Use HTTP/2 only server for h2c support
    loop {
        let (stream, _addr) = listener.accept().await?;
        let io = TokioIo::new(stream);

        // Convert axum router to hyper service
        let service = TowerToHyperService::new(router.clone());

        tokio::spawn(async move {
            if let Err(err) = http2::Builder::new(TokioExecutor::new())
                .serve_connection(io, service)
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
