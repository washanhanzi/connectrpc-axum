# HTTP Endpoints

Serve regular HTTP endpoints alongside ConnectRPC services on the same server.

## Overview

Since connectrpc-axum is built on Axum, you can combine ConnectRPC services with regular HTTP routes. This is useful for:

- Adding REST endpoints alongside RPC
- Supporting legacy HTTP APIs
- Health checks and metrics endpoints

## Sharing Handlers

A single handler can serve both HTTP and ConnectRPC endpoints using `post_connect`:

```rust
use axum::{Router, extract::State, routing::get};
use connectrpc_axum::prelude::*;

#[derive(Clone)]
struct AppState;

async fn health_check() -> &'static str {
    "OK"
}

// Single handler works for both routes
async fn get_user(
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<user::v1::GetUserRequest>,
) -> Result<ConnectResponse<user::v1::GetUserResponse>, ConnectError> {
    Ok(ConnectResponse(user::v1::GetUserResponse {
        name: format!("User {}", req.id),
    }))
}

// HTTP routes using post_connect
let http_router = Router::new()
    .route("/health", get(health_check))
    .route("/api/user", post_connect(get_user))
    .with_state(AppState);

// ConnectRPC router with full protocol support
let connect_router = connectrpc_axum::MakeServiceBuilder::new()
    .add_router(
        userservice::UserServiceBuilder::new()
            .get_user(get_user)
            .with_state(AppState)
            .build()
    )
    .build();

// Merge both routers
let app = http_router.merge(connect_router);
```

This serves both paths:
- `POST /api/user` - HTTP endpoint
- `POST /user.v1.UserService/GetUser` - ConnectRPC endpoint


### GET Support for Idempotent RPCs

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