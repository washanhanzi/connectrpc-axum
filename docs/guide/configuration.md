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

### Message Limits

Set maximum message size for requests and responses:

```rust
use connectrpc_axum::MessageLimits;

// Default is 4MB
let limits = MessageLimits::new(16 * 1024 * 1024);  // 16MB

MakeServiceBuilder::new()
    .add_router(router)
    .message_limits(limits)
    .build()
```

### Timeout

Set server-side maximum timeout. Works with client `Connect-Timeout-Ms` header:

```rust
use std::time::Duration;

MakeServiceBuilder::new()
    .add_router(router)
    .timeout(Duration::from_secs(30))
    .build()
```

| Scenario | Effective Timeout |
|----------|-------------------|
| Client sends `Connect-Timeout-Ms: 5000` | 5 seconds |
| Server sets `.timeout(30s)` | 30 seconds |
| Both (client: 5s, server: 30s) | 5 seconds (minimum) |
| Both (client: 60s, server: 30s) | 30 seconds (minimum) |

### Protocol Header Validation

Require the `Connect-Protocol-Version` header for Connect protocol requests:

```rust
MakeServiceBuilder::new()
    .add_router(router)
    .require_protocol_header(true)  // Reject requests without header
    .build()
```

## Adding gRPC Support

With the `tonic` feature, serve both Connect and gRPC on the same port:

```rust
// Use TonicCompatibleBuilder for dual-protocol support
let (connect_router, grpc_service) =
    helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
        .say_hello(say_hello)
        .with_state(AppState::default())
        .build();

let service = MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_grpc_service(grpc_service)  // Routes application/grpc* to Tonic
    .timeout(Duration::from_secs(30))
    .build();
```

Requests are routed by `Content-Type`:
- `application/grpc*` → Tonic gRPC server
- Otherwise → Axum routes (Connect protocol)

### Disabling FromRequestParts Extraction

By default, `FromRequestPartsLayer` is applied to gRPC services to enable axum's `FromRequestParts` extractors in handlers. If your handlers don't use extractors, disable this to avoid the overhead:

```rust
let service = MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_grpc_service(grpc_service)
    .without_from_request_parts()  // Disable extractor support
    .build();
```
