//! Example 4: Tonic Server Streaming (also works with ConnectRPC)
//!
//! This example demonstrates dual-protocol server streaming:
//! - Server streaming RPC works with both Connect and gRPC protocols
//! - Uses TonicCompatibleBuilder for both routers
//! - Same streaming handler serves both protocols
//!
//! Run with: cargo run --bin tonic-server-stream
//! Test with:
//!   - Connect: go run ./cmd/client --protocol connect server-stream
//!   - gRPC:    go run ./cmd/client --protocol grpc server-stream

use axum::extract::State;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use futures::Stream;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::AtomicUsize};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

/// Streaming handler with state - works for both Connect and gRPC
async fn say_hello_stream(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    let hobbies = req.hobbies;
    let counter = state.counter.clone();

    let response_stream = async_stream::stream! {
        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("Stream #{}: Hello, {}!", count, name),
            response_type: None,
        });

        if !hobbies.is_empty() {
            for (idx, hobby) in hobbies.iter().enumerate() {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                yield Ok(HelloResponse {
                    message: format!("Stream #{}: Hobby {}: {}", count, idx + 1, hobby),
                    response_type: None,
                });
            }
        } else {
            for i in 1..=3 {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                yield Ok(HelloResponse {
                    message: format!("Stream #{}: Message {} for {}", count, i, name),
                    response_type: None,
                });
            }
        }

        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("Stream #{}: Goodbye, {}!", count, name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build both Connect router and gRPC server from same handlers
    let (connect_router, grpc_server) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello_stream(say_hello_stream)
            .with_state(app_state)
            .build();

    // Combine into a single service that routes by Content-Type
    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example 4: Tonic Server Streaming (+ ConnectRPC) ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHelloStream (server streaming): /hello.HelloWorldService/SayHelloStream");
    println!();
    println!("Protocols supported:");
    println!("  - Connect (JSON): Content-Type: application/connect+json");
    println!("  - Connect (Proto): Content-Type: application/connect+proto");
    println!("  - gRPC: Content-Type: application/grpc");
    println!();
    println!("Test with:");
    println!("  go run ./cmd/client --protocol connect server-stream");
    println!("  go run ./cmd/client --protocol grpc server-stream");

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
