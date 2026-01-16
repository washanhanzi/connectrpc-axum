# Message Limits

Configure size limits for incoming requests and outgoing responses to protect against memory exhaustion and oversized payloads.

## Configuration

```rust
use connectrpc_axum::{MakeServiceBuilder, MessageLimits};

// Default: 4MB receive limit, no send limit
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

## Receive Limit

`receive_max_bytes` limits the size of incoming request messages. This protects the server from clients sending oversized requests that could exhaust memory.

| Setting | Default | Error |
|---------|---------|-------|
| `receive_max_bytes` | 4 MB | `ResourceExhausted` |

When exceeded, the server returns a `ResourceExhausted` error before processing the request.

### Axum Router Behavior

When you add plain HTTP routes via `add_axum_router()`, the receive limit is applied using Tower's `RequestBodyLimitLayer`. This provides consistent size limiting across your entire service.

| Route Type | Error Response |
|------------|----------------|
| Connect routes | `ResourceExhausted` (JSON error) |
| Axum routes | `413 Payload Too Large` (HTTP status) |

::: tip
Both route types respect the same `receive_max_bytes` configuration, but return errors appropriate to their protocol.
:::

## Send Limit

`send_max_bytes` limits the size of outgoing response messages. This prevents the server from accidentally sending oversized responses that could overwhelm clients.

| Setting | Default | Error |
|---------|---------|-------|
| `send_max_bytes` | Unlimited | `ResourceExhausted` |

```rust
// Convenience method for setting send limit only
MakeServiceBuilder::new()
    .add_router(router)
    .send_max_bytes(8 * 1024 * 1024)  // 8MB
    .build()
```

### Compression Interaction

Following connect-go's behavior, the send size is checked **after** encoding and compression. This means:

- If compression reduces the message below the limit, it succeeds
- If compression is not applied (message too small or disabled), the uncompressed size is checked
- Error messages indicate whether the checked size was compressed or not

### Streaming

For streaming responses, each message is checked individually. If a message exceeds the limit:

1. All previous messages are delivered successfully
2. The oversized message triggers a `ResourceExhausted` error
3. The stream terminates with the error

## Unlimited Mode

For trusted environments where size limits are not needed:

```rust
let limits = MessageLimits::unlimited();
```

::: warning
Using unlimited message sizes can allow memory exhaustion attacks. Only use this in trusted environments.
:::
