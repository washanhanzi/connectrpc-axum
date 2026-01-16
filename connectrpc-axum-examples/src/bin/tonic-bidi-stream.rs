//! Example 5: Tonic Bidirectional Streaming (gRPC only)
//!
//! This example demonstrates bidirectional streaming:
//! - Client and server can send messages independently
//! - Only supported by gRPC (not Connect protocol)
//! - Uses custom Tonic service implementation
//!
//! Run with: cargo run --bin tonic-bidi-stream
//! Test with: go run ./cmd/client --protocol grpc bidi-stream

use axum::extract::State;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{
    EchoRequest, EchoResponse, HelloRequest, HelloResponse, echo_service_server, helloworldservice,
};
use futures::{Stream, StreamExt};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, atomic::AtomicUsize};
use tonic::Status;

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

// ============================================================================
// Echo Service - Custom Tonic Implementation for Bidi Streaming
// ============================================================================

struct EchoServiceImpl {
    app_state: AppState,
}

#[tonic::async_trait]
impl echo_service_server::EchoService for EchoServiceImpl {
    /// Unary RPC
    async fn echo(
        &self,
        request: tonic::Request<EchoRequest>,
    ) -> Result<tonic::Response<EchoResponse>, Status> {
        let req = request.into_inner();
        let count = self
            .app_state
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(tonic::Response::new(EchoResponse {
            message: format!("Echo #{}: {}", count, req.message),
        }))
    }

    /// Client streaming RPC - collect all messages then respond
    async fn echo_client_stream(
        &self,
        request: tonic::Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<tonic::Response<EchoResponse>, Status> {
        let mut stream = request.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(req) => messages.push(req.message),
                Err(e) => return Err(e),
            }
        }

        let count = self
            .app_state
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(tonic::Response::new(EchoResponse {
            message: format!(
                "Client Stream #{}: Received {} messages: [{}]",
                count,
                messages.len(),
                messages.join(", ")
            ),
        }))
    }

    /// Bidirectional streaming RPC - echo each message as it arrives
    type EchoBidiStreamStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

    async fn echo_bidi_stream(
        &self,
        request: tonic::Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<tonic::Response<Self::EchoBidiStreamStream>, Status> {
        let mut stream = request.into_inner();
        let counter = self.app_state.counter.clone();

        let response_stream = async_stream::stream! {
            let mut message_count = 0;

            while let Some(result) = stream.next().await {
                match result {
                    Ok(req) => {
                        message_count += 1;
                        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        yield Ok(EchoResponse {
                            message: format!(
                                "Bidi Echo #{} (msg #{}): {}",
                                count, message_count, req.message
                            ),
                        });
                    }
                    Err(e) => {
                        yield Err(e);
                        break;
                    }
                }
            }

            let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            yield Ok(EchoResponse {
                message: format!(
                    "Bidi stream #{} completed. Received {} messages.",
                    count, message_count
                ),
            });
        };

        Ok(tonic::Response::new(Box::pin(response_stream)))
    }
}

// ============================================================================
// Hello Service - Connect Handler for comparison
// ============================================================================

async fn say_hello(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state
        .counter
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello #{}, {}!", count, req.name.unwrap_or_default()),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build Connect router for HelloWorldService (unary only, for comparison)
    let hello_router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .with_state(app_state.clone())
        .build();

    // Build Tonic gRPC service for EchoService (with bidi streaming)
    let echo_grpc_service = echo_service_server::EchoServiceServer::new(EchoServiceImpl {
        app_state: app_state.clone(),
    });

    // Combine services
    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_router)
        .add_grpc_service(echo_grpc_service)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example 5: Tonic Bidirectional Streaming (gRPC only) ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Services:");
    println!("  HelloWorldService (Connect):");
    println!("    - SayHello (unary)");
    println!();
    println!("  EchoService (gRPC only):");
    println!("    - Echo (unary)");
    println!("    - EchoClientStream (client streaming)");
    println!("    - EchoBidiStream (bidirectional streaming)");
    println!();
    println!("Test with:");
    println!("  go run ./cmd/client --protocol grpc bidi-stream");

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
