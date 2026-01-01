# HTTP Endpoints

Serve regular HTTP endpoints alongside ConnectRPC services on the same server.

## Overview

Since connectrpc-axum is built on Axum, you can combine ConnectRPC services with regular HTTP routes. This is useful for:

- Adding REST endpoints alongside RPC
- Supporting legacy HTTP APIs
- Health checks and metrics endpoints

## Basic Example

```rust
use axum::{Router, routing::get};
use connectrpc_axum::prelude::*;

// ConnectRPC handler
async fn get_user(
    ConnectRequest(req): ConnectRequest<GetUserRequest>,
) -> Result<ConnectResponse<GetUserResponse>, ConnectError> {
    Ok(ConnectResponse(GetUserResponse {
        name: format!("User {}", req.id),
    }))
}

// Regular HTTP handler
async fn health_check() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build ConnectRPC router
    let connect_router = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(
            userservice::UserServiceBuilder::new()
                .get_user(get_user)
                .build()
        )
        .build();

    // Combine with HTTP routes
    let app = Router::new()
        .route("/health", get(health_check))
        .merge(connect_router);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

## Sharing Handlers

A single handler can serve both HTTP and ConnectRPC endpoints since Connect protocol supports JSON encoding:

```rust
use axum::{Router, routing::post, extract::State};
use connectrpc_axum::prelude::*;

#[derive(Clone)]
struct AppState;

// Single handler works for both routes
async fn get_user(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<user::v1::GetUserRequest>,
) -> Result<ConnectResponse<user::v1::GetUserResponse>, ConnectError> {
    Ok(ConnectResponse(user::v1::GetUserResponse {
        name: format!("User {}", req.id),
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build ConnectRPC router
    let connect_router = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(
            userservice::UserServiceBuilder::new()
                .get_user(get_user)
                .with_state(AppState)
                .build()
        )
        .build();

    // Serve both paths with the same handler
    let app = Router::new()
        .route("/api/user", post(get_user))  // REST endpoint
        .merge(connect_router)                // ConnectRPC endpoint
        .with_state(AppState);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

This serves both paths with JSON:
- `POST /api/user` - REST endpoint
- `POST /user.v1.UserService/GetUser` - ConnectRPC endpoint

## Adding Middleware

Apply Axum middleware to specific routes or the entire app:

```rust
use axum::{Router, middleware};
use tower_http::cors::CorsLayer;

let connect_router = connectrpc_axum::MakeServiceBuilder::new()
    .add_router(service_router)
    .build();

let app = Router::new()
    .route("/health", get(health_check))
    .merge(connect_router)
    .layer(CorsLayer::permissive());  // Apply to all routes
```
