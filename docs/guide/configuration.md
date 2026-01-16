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

Configure size limits for incoming requests and outgoing responses:

```rust
use connectrpc_axum::MessageLimits;

// Default is 4MB receive limit, no send limit
let limits = MessageLimits::default();

// Custom receive limit only
let limits = MessageLimits::new(16 * 1024 * 1024);  // 16MB

// Custom limits for both directions
let limits = MessageLimits::default()
    .receive_max_bytes(16 * 1024 * 1024)  // 16MB for requests
    .send_max_bytes(8 * 1024 * 1024);     // 8MB for responses

MakeServiceBuilder::new()
    .add_router(router)
    .message_limits(limits)
    .build()
```

#### Receive Limit (`receive_max_bytes`)

Limits the size of incoming request messages. Protects against clients sending oversized requests that could exhaust server memory.

#### Send Limit (`send_max_bytes`)

Limits the size of outgoing response messages. Prevents the server from accidentally sending oversized responses that could overwhelm clients. When exceeded, returns a `ResourceExhausted` error.

```rust
// Convenience method for setting send limit only
MakeServiceBuilder::new()
    .add_router(router)
    .send_max_bytes(8 * 1024 * 1024)  // 8MB
    .build()
```

Following connect-go's behavior, the send size is checked after encoding and after compression (if applied).

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
