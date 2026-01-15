# HTTP Endpoints

Serve regular HTTP endpoints alongside ConnectRPC services on the same server.

## Overview

Since connectrpc-axum is built on Axum, you can combine ConnectRPC services with regular HTTP routes. This is useful for:

- Adding REST endpoints alongside RPC
- Supporting legacy HTTP APIs
- Health checks and metrics endpoints

## Example

Use `MakeServiceBuilder` to combine ConnectRPC services with plain HTTP routes:

- `add_router()` - ConnectRPC routes (with `ConnectLayer` for protocol handling)
- `add_axum_router()` - Plain HTTP routes (bypass `ConnectLayer`)

You can also use `post_connect` to expose a ConnectRPC handler at a custom HTTP path.

```rust
use axum::{Router, routing::{get, post}, Json};
use connectrpc_axum::prelude::*;
use connectrpc_axum::MakeServiceBuilder;
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

// Plain HTTP handler - returns JSON
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

// Plain HTTP handler - returns text
async fn metrics() -> &'static str {
    "requests_total 42\nrequest_errors 0"
}

// ConnectRPC handler - can be used for both RPC and custom HTTP routes
async fn get_user(
    ConnectRequest(req): ConnectRequest<user::v1::GetUserRequest>,
) -> Result<ConnectResponse<user::v1::GetUserResponse>, ConnectError> {
    Ok(ConnectResponse::new(user::v1::GetUserResponse {
        name: format!("User {}", req.id),
    }))
}

// ConnectRPC service router
let connect_router = userservice::UserServiceBuilder::new()
    .get_user(get_user)
    .build();

// Plain HTTP routes (health, metrics, and a custom path for the RPC handler)
let axum_router = Router::new()
    .route("/health", get(health))
    .route("/metrics", get(metrics))
    .route("/api/user", post_connect(get_user));  // Same handler at custom path

// Combine them
let app = MakeServiceBuilder::new()
    .add_router(connect_router)      // ConnectRPC routes get ConnectLayer
    .add_axum_router(axum_router)    // Plain routes bypass ConnectLayer
    .build();

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, app).await?;
```

This serves:
- `POST /user.v1.UserService/GetUser` - ConnectRPC endpoint (with protocol handling)
- `POST /api/user` - Same handler at custom HTTP path
- `GET /health` - Plain JSON endpoint (no Connect headers required)
- `GET /metrics` - Plain text endpoint

## GET Support for Idempotent RPCs

Use `get_connect` to enable GET requests for idempotent unary RPCs. This allows browser caching:

```rust
// Support both GET and POST
let app = Router::new()
    .route("/user.v1.UserService/GetUser",
        get_connect(get_user).merge(post_connect(get_user)));
```

GET requests encode the message in query parameters:
- `encoding=json|proto` (required)
- `message=<payload>` (required, URL-encoded)
- `base64=1` (optional, for binary payloads)
- `compression=gzip|identity` (optional)