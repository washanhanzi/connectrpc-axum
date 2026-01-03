//! Example 6: gRPC-Web via tonic-web
//!
//! This example demonstrates gRPC-Web support:
//! - Enables browser-based clients to call gRPC services
//! - Uses tonic-web layer for HTTP/1.1 compatibility
//! - Supports Connect, gRPC, and gRPC-Web protocols on the same port
//!
//! Run with: cargo run --bin grpc-web --features tonic-web
//! Test with: go run ./cmd/client grpc-web

use axum::extract::State;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{helloworldservice, HelloRequest, HelloResponse};
use futures::Stream;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicUsize, Arc};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

/// Unary RPC handler
async fn say_hello(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state
        .counter
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = req.name.unwrap_or_else(|| "World".to_string());

    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello #{}, {}! (via gRPC-Web)", count, name),
        response_type: None,
    }))
}

/// Server streaming RPC handler
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

    let stream = async_stream::stream! {
        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("gRPC-Web Stream #{}: Hello, {}!", count, name),
            response_type: None,
        });

        if !hobbies.is_empty() {
            for (idx, hobby) in hobbies.iter().enumerate() {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                yield Ok(HelloResponse {
                    message: format!("gRPC-Web Stream #{}: Hobby {}: {}", count, idx + 1, hobby),
                    response_type: None,
                });
            }
        } else {
            for i in 1..=3 {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                yield Ok(HelloResponse {
                    message: format!("gRPC-Web Stream #{}: Message {}", count, i),
                    response_type: None,
                });
            }
        }

        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("gRPC-Web Stream #{}: Goodbye!", count),
            response_type: None,
        });
    };

    Ok(ConnectResponse(StreamBody::new(stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build both Connect router and gRPC server from same handlers
    let (connect_router, grpc_service) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .say_hello_stream(say_hello_stream)
            .with_state(app_state)
            .build();

    // Wrap gRPC server with gRPC-Web layer
    let grpc_web_server = tower::ServiceBuilder::new()
        .layer(tonic_web::GrpcWebLayer::new())
        .service(grpc_service);

    // Combine with MakeServiceBuilder
    let service = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_web_server)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example 6: gRPC-Web via tonic-web ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Service: HelloWorldService");
    println!("  - SayHello (unary)");
    println!("  - SayHelloStream (server streaming)");
    println!();
    println!("Protocols supported:");
    println!("  - Connect: Content-Type: application/json, application/proto");
    println!("  - gRPC: Content-Type: application/grpc");
    println!("  - gRPC-Web: Content-Type: application/grpc-web");
    println!();
    println!("Test with:");
    println!("  go run ./cmd/client grpc-web");

    axum::serve(listener, tower::make::Shared::new(service)).await?;
    Ok(())
}
