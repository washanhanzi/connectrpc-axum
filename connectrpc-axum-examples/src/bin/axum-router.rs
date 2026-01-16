//! Example: MakeServiceBuilder with axum router
//!
//! This example demonstrates using `add_axum_router()` to add plain axum routes
//! that bypass ConnectLayer. Useful for:
//! - Health check endpoints
//! - Metrics endpoints
//! - Static file serving
//! - Plain REST APIs
//!
//! Run with: cargo run --bin axum-router
//! Test with Go client: go test -v -run TestAxumRouter

use axum::{Json, Router, routing::get};
use connectrpc_axum::MakeServiceBuilder;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use serde::Serialize;
use std::net::SocketAddr;

/// Simple stateless handler for unary RPC
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// Plain axum handler - returns JSON without Connect protocol handling
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Plain axum handler - returns plain text
async fn metrics() -> &'static str {
    "requests_total 42\nrequests_errors 0"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build Connect router for RPC services (without layer - MakeServiceBuilder will apply it)
    let connect_router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    // Build plain axum router for health/metrics endpoints
    let axum_router = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics));

    // Combine them using MakeServiceBuilder
    // - connect_router goes through ConnectLayer
    // - axum_router bypasses ConnectLayer
    let app = MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_axum_router(axum_router)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Example: MakeServiceBuilder with axum router ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Connect endpoints (with ConnectLayer):");
    println!("  POST /hello.HelloWorldService/SayHello");
    println!();
    println!("Plain axum endpoints (bypass ConnectLayer):");
    println!("  GET /health   - returns JSON health status");
    println!("  GET /metrics  - returns plain text metrics");
    println!();
    println!("Test with:");
    println!("  curl http://localhost:3000/health");
    println!("  curl http://localhost:3000/metrics");
    println!("  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHello \\");
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"name\": \"Alice\"}}'");

    axum::serve(listener, app).await?;
    Ok(())
}
