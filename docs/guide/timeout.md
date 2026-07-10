# Timeout

## Basic Usage

Set server-side maximum timeout using `MakeServiceBuilder`:

```rust
use std::time::Duration;

MakeServiceBuilder::new()
    .add_router(router)
    .timeout(Duration::from_secs(30))
    .build()
```

## Connect-Timeout-Ms Header

The server respects the client's `Connect-Timeout-Ms` header. When both client and server timeouts are configured, the smaller value wins:

| Scenario | Effective Timeout |
|----------|-------------------|
| Client sends `Connect-Timeout-Ms: 5000` | 5 seconds |
| Server sets `.timeout(30s)` | 30 seconds |
| Both (client: 5s, server: 30s) | 5 seconds (minimum) |
| Both (client: 60s, server: 30s) | 30 seconds (minimum) |

## Axum Router Timeout

When you add plain HTTP routes via `add_axum_router()`, they also receive the configured timeout via Tower's `TimeoutLayer`. Unlike Connect routes which return a `deadline_exceeded` error, plain HTTP routes return `408 Request Timeout`.

```rust
let axum_router = Router::new()
    .route("/health", get(health_handler));

MakeServiceBuilder::new()
    .add_router(connect_router)
    .add_axum_router(axum_router)  // Also gets 30s timeout
    .timeout(Duration::from_secs(30))
    .build()
```

## Implementation Details

The timeout is applied via `ConnectLayer` using one absolute deadline for handler execution and response streaming. When the timeout is exceeded, a proper Connect protocol `deadline_exceeded` error is returned.

```rust
// Simplified implementation
match tokio::time::timeout_at(deadline, handler.call(req)).await {
    Ok(result) => result,
    Err(_elapsed) => ConnectError::new(Code::DeadlineExceeded, "request timeout exceeded"),
}
```

### Streaming RPCs

The timeout covers the complete RPC lifecycle:

- **Unary and client-streaming RPCs**: The request and single response must complete before the deadline.
- **Server-streaming and bidirectional RPCs**: Handler execution and response stream consumption share the same deadline. If it expires after response headers are sent, the server emits a `deadline_exceeded` EndStream frame and stops the response body.

Clients enforce the same absolute deadline locally, so an unresponsive or non-conforming server cannot keep a response stream open indefinitely.

## Avoid Using Axum's TimeoutLayer Directly

Do not apply `TimeoutLayer` manually on Connect routes:

```rust
// DON'T do this for Connect routes
use tower_http::timeout::TimeoutLayer;

let service = MakeServiceBuilder::new()
    .add_router(router)
    .build()
    .layer(TimeoutLayer::new(Duration::from_secs(30)));  // Wrong!
```

This returns a generic HTTP error instead of Connect's `deadline_exceeded` error code, breaking protocol compliance.

Always use `.timeout()` on `MakeServiceBuilder` - it applies the correct timeout behavior to both Connect and plain HTTP routes.
