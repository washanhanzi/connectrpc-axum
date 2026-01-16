# MakeServiceBuilder

## Service Builders

Generated builders register handlers and create bare routers:

```rust
// Build bare routers (no ConnectLayer applied yet)
let hello_router = helloworldservice::HelloWorldServiceBuilder::new()
    .say_hello(say_hello)
    .say_hello_stream(say_hello_stream)
    .with_state(AppState::default())
    .build();  // Returns Router without ConnectLayer

let user_router = userservice::UserServiceBuilder::new()
    .get_user(get_user)
    .with_state(AppState::default())
    .build();
```

::: tip
Use `build()` when combining multiple services or configuring options.
Use `build_connect()` only for simple single-service setups.
:::

## MakeServiceBuilder

Combines routers and applies protocol configuration:

```rust
use std::time::Duration;
use connectrpc_axum::{MakeServiceBuilder, MessageLimits};

let service = MakeServiceBuilder::new()
    // Add service routers
    .add_router(hello_router)
    .add_router(user_router)

    // Configure options
    .message_limits(MessageLimits::new(16 * 1024 * 1024))  // 16MB max
    .timeout(Duration::from_secs(30))
    .require_protocol_header(true)

    // Build final service
    .build();

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, tower::make::Shared::new(service)).await?;
```

## Configuration Options

Configuration options apply to:
- `add_router()` — Connect routers
- `add_axum_router()` — Axum routers with shared config

Configuration options do **not** apply to:
- `add_axum_router_raw()` — Axum routers without any layers
- `add_grpc_service()` — gRPC services (configure compression via Tonic directly)

### Message Limits

See [Message Limits](./limits.md) for configuring receive and send size limits.

### Timeout

See [Timeout](./timeout.md) for detailed timeout configuration.

### Compression

See [Compression](./compression.md) for detailed compression configuration.

### Protocol Header Validation

Require the `Connect-Protocol-Version` header for Connect protocol requests:

```rust
MakeServiceBuilder::new()
    .add_router(router)
    .require_protocol_header(true)  // Reject requests without header
    .build()
```

## Adding gRPC Support

See [Tonic gRPC Integration](./tonic.md) for serving both Connect and gRPC on the same port.

## Adding Axum Routers

See [Axum Router Integration](./axum-router.md) for combining plain HTTP routes with ConnectRPC services.
