//! Example: Tonic with multiple FromRequestParts extractors
//!
//! This example demonstrates using multiple extractors with TonicCompatibleBuilder:
//! - Custom ApiKey extractor (extracts x-api-key header)
//! - State extractor
//! - ConnectRequest
//!
//! Works with both Connect and gRPC protocols.
//!
//! Run with: cargo run --bin tonic-extractor
//! Test with:
//!   - Connect: curl with x-api-key header
//!   - gRPC: grpcurl with x-api-key metadata

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
// SocketAddr now provided by server_addr()
use std::sync::{Arc, atomic::AtomicUsize};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

/// Custom extractor that validates the x-api-key header.
/// Returns `ConnectError` on rejection.
pub struct ApiKey(pub String);

impl<S> FromRequestParts<S> for ApiKey
where
    S: Send + Sync,
{
    type Rejection = ConnectError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| ApiKey(s.to_string()))
            .ok_or_else(|| ConnectError::new_unauthenticated("missing x-api-key header"))
    }
}

/// Handler with multiple extractors: ApiKey, State, and ConnectRequest
async fn say_hello(
    ApiKey(api_key): ApiKey,
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state
        .counter
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = req.name.unwrap_or_else(|| "World".to_string());

    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello #{}, {}! (api_key: {})", count, name, api_key),
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build both Connect router and gRPC server from same handlers
    let (connect_router, grpc_server) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .with_state(app_state)
            .build();

    // Combine into a single service that routes by Content-Type
    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example: Tonic with Multiple Extractors ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Handler signature: (ApiKey, State<AppState>, ConnectRequest<HelloRequest>)");
    println!();
    println!("Test WITHOUT x-api-key (should fail with UNAUTHENTICATED):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");
    println!();
    println!("Test WITH x-api-key (should succeed):");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -H 'x-api-key: secret123' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
