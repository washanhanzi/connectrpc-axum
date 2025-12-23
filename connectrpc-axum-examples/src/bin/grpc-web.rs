//! Example 6: gRPC-Web via tonic-web
//!
//! This example demonstrates gRPC-Web support:
//! - Enables browser-based clients to call gRPC services
//! - Uses tonic-web layer for HTTP/1.1 compatibility
//! - Supports both gRPC and gRPC-Web protocols
//!
//! Run with: cargo run --bin grpc-web --features tonic-web
//! Test with: go run ./cmd/client grpc-web

use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_server};
use futures::Stream;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, atomic::AtomicUsize};
use tonic::Status;

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

// Custom Tonic implementation for gRPC-Web
struct HelloServiceImpl {
    app_state: AppState,
}

#[tonic::async_trait]
impl hello_world_service_server::HelloWorldService for HelloServiceImpl {
    /// Unary RPC
    async fn say_hello(
        &self,
        request: tonic::Request<HelloRequest>,
    ) -> Result<tonic::Response<HelloResponse>, Status> {
        let req = request.into_inner();
        let count = self.app_state.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = req.name.unwrap_or_else(|| "World".to_string());

        Ok(tonic::Response::new(HelloResponse {
            message: format!("Hello #{}, {}! (via gRPC-Web)", count, name),
            response_type: None,
        }))
    }

    /// Server streaming RPC
    type SayHelloStreamStream = Pin<Box<dyn Stream<Item = Result<HelloResponse, Status>> + Send>>;

    async fn say_hello_stream(
        &self,
        request: tonic::Request<HelloRequest>,
    ) -> Result<tonic::Response<Self::SayHelloStreamStream>, Status> {
        let req = request.into_inner();
        let name = req.name.unwrap_or_else(|| "World".to_string());
        let hobbies = req.hobbies;
        let counter = self.app_state.counter.clone();

        let response_stream = async_stream::stream! {
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

        Ok(tonic::Response::new(Box::pin(response_stream)))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build Tonic gRPC service
    let grpc_service = hello_world_service_server::HelloWorldServiceServer::new(
        HelloServiceImpl { app_state },
    );

    // Build Routes and wrap with GrpcWebLayer for gRPC-Web support
    let grpc_routes = tonic::service::Routes::new(grpc_service);

    // Apply GrpcWebLayer using tower's ServiceBuilder
    let grpc_web_service = tower::ServiceBuilder::new()
        .layer(tonic_web::GrpcWebLayer::new())
        .service(grpc_routes.prepare());

    // Create a minimal Connect router (empty for this example)
    let connect_router = axum::Router::new();

    // Create the content-type switch dispatcher
    let dispatch = connectrpc_axum::tonic::ContentTypeSwitch::new(
        grpc_web_service,
        connect_router,
    );

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
    println!("  - gRPC: Content-Type: application/grpc");
    println!("  - gRPC-Web: Content-Type: application/grpc-web");
    println!("  - gRPC-Web+Proto: Content-Type: application/grpc-web+proto");
    println!();
    println!("Test with:");
    println!("  go run ./cmd/client grpc-web");

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
