# Axum Router Integration

Combine Axum routers with ConnectRPC services in `MakeServiceBuilder`.

## Router Methods

| Method | Layers Applied |
|--------|----------------|
| `add_axum_router()` / `add_axum_routers()` | Compression + timeout + limits from `MakeServiceBuilder` |
| `add_axum_router_raw()` / `add_axum_routers_raw()` | None |

## add_axum_router / add_axum_routers

Adds Axum routers that inherit the compression, timeout, and message limit configuration from `MakeServiceBuilder`:

```rust
use std::time::Duration;
use axum::{Router, routing::get};
use connectrpc_axum::MakeServiceBuilder;

let rest_router = Router::new()
    .route("/api/users", get(list_users))
    .route("/api/users/:id", get(get_user_by_id));

let app = MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_axum_router(rest_router)  // Inherits timeout, compression, limits
    .timeout(Duration::from_secs(30))
    .compression(CompressionConfig::new(1024))
    .receive_max_bytes(4 * 1024 * 1024)
    .build();
```

::: tip
Timeout behavior differs by route type: `add_axum_router()` returns `408 Request Timeout`, while Connect routes return `deadline_exceeded`.
:::

## add_axum_router_raw / add_axum_routers_raw

Adds Axum routers without any middleware layers. Use this when you want full control over the router configuration:

```rust
use axum::{Router, routing::get};
use connectrpc_axum::MakeServiceBuilder;

let health_router = Router::new()
    .route("/health", get(|| async { "ok" }))
    .route("/metrics", get(|| async { "metrics_total 42" }));

let app = MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_axum_router_raw(health_router)  // No layers applied
    .build();
```

## post_connect / get_connect

Transform a Connect handler into an Axum-compatible handler for custom routing:

```rust
use axum::{Router, routing::post};
use connectrpc_axum::prelude::*;

async fn get_user(
    ConnectRequest(req): ConnectRequest<GetUserRequest>,
) -> Result<ConnectResponse<GetUserResponse>, ConnectError> {
    Ok(ConnectResponse::new(GetUserResponse {
        name: format!("User {}", req.id),
    }))
}

// Register in service builder AND at custom path
let connect_router = userservice::UserServiceBuilder::new()
    .get_user(get_user)
    .build();

let custom_router = Router::new()
    .route("/api/user", post_connect(get_user));

let app = MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_axum_router_raw(custom_router)
    .build();
```

This serves:
- `POST /user.v1.UserService/GetUser` — ConnectRPC endpoint
- `POST /api/user` — Same handler at custom path

### GET Requests

Use `get_connect` for idempotent RPCs to enable browser caching:

```rust
use axum::Router;
use connectrpc_axum::prelude::*;

let router = Router::new()
    .route("/user.v1.UserService/GetUser",
        get_connect(get_user).merge(post_connect(get_user)));
```

GET requests encode the message in query parameters:

| Parameter | Required | Description |
|-----------|----------|-------------|
| `encoding` | Yes | `json` or `proto` |
| `message` | Yes | URL-encoded payload |
| `base64` | No | Set to `1` for binary payloads |
| `compression` | No | `gzip` or `identity` |
