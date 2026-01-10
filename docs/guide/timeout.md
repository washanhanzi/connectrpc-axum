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

## Implementation Details

The timeout is applied via `ConnectLayer`, which wraps the handler future with `tokio::time::timeout`. When the timeout is exceeded, a proper Connect protocol `deadline_exceeded` error is returned.

```rust
// Simplified implementation
match tokio::time::timeout(duration, handler.call(req)).await {
    Ok(result) => result,
    Err(_elapsed) => ConnectError::new(Code::DeadlineExceeded, "request timeout exceeded"),
}
```

### Streaming RPCs

The timeout applies only to handler execution, not to response body streaming. This means:

- **Unary RPCs**: Timeout covers the entire request-response cycle
- **Server-streaming RPCs**: Timeout covers handler execution until the response headers are sent; the stream body can continue indefinitely after

This differs from connect-go's full lifecycle timeout but suits long-lived streaming scenarios where streams may intentionally run longer than typical request timeouts.

::: warning
If you need to enforce deadlines on streaming bodies, implement timeout logic within your stream handler.
:::

## Avoid Using Axum's TimeoutLayer

Do not use Axum's standard `TimeoutLayer` for Connect protocol timeouts:

```rust
// DON'T do this
use tower_http::timeout::TimeoutLayer;

let service = MakeServiceBuilder::new()
    .add_router(router)
    .build()
    .layer(TimeoutLayer::new(Duration::from_secs(30)));  // Wrong!
```

This returns a generic HTTP error instead of Connect's `deadline_exceeded` error code, breaking protocol compliance.

Always use:
- `.timeout()` on `MakeServiceBuilder`, or
- `ConnectTimeoutLayer` directly
