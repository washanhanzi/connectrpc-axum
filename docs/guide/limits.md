# Message Limits

Configure size limits for incoming requests and outgoing responses to protect against memory exhaustion and oversized payloads.

## Configuration

```rust
use connectrpc_axum::{MakeServiceBuilder, MessageLimits};

// Default: no limits
let limits = MessageLimits::new();

// Set receive limit only
let limits = MessageLimits::new()
    .receive_max_bytes(16 * 1024 * 1024);  // 16MB

// Set both receive and send limits
let limits = MessageLimits::new()
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
| `receive_max_bytes` | No limit | `ResourceExhausted` |

When exceeded, the server returns a `ResourceExhausted` error before processing the request.

For streaming requests, `receive_max_bytes` also bounds decompression itself: compressed envelopes are decompressed with the limit enforced as output is produced, so a small compressed frame (a "decompression bomb") cannot expand past the limit. Exceeding the limit during decompression returns `ResourceExhausted`, while a frame that fails to decompress returns `InvalidArgument`.

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
| `send_max_bytes` | No limit | `ResourceExhausted` |

```rust
// Convenience method for setting send limit only
MakeServiceBuilder::new()
    .add_router(router)
    .send_max_bytes(8 * 1024 * 1024)  // 8MB
    .build()
```

### Compression Interaction

The point at which the send size is checked differs between unary and streaming RPCs:

- **Unary**: the limit is checked on the **uncompressed** encoded message, before Tower's `CompressionLayer` compresses the response body. An over-limit message is rejected even if compression would have brought it under the limit. The error always reads `message size {size} exceeds sendMaxBytes {limit}`.
- **Streaming**: each envelope is compressed first (when envelope compression is negotiated), and the limit is checked on the **compressed** envelope size. Compression can therefore bring a message under the limit. The error message indicates whether the checked size was compressed (`compressed message size ...`) or not (`message size ...`).

### Streaming

For streaming responses, each message is checked individually. If a message exceeds the limit:

1. All previous messages are delivered successfully
2. The oversized message triggers a `ResourceExhausted` error
3. The stream terminates with the error

If the final EndStream control frame would exceed `send_max_bytes` because it
contains large error details, connectrpc-axum retries with a reduced EndStream
frame that strips the details. If even that reduced frame is still too large,
it is sent anyway so the client still receives the error code.

This is a deliberate divergence from the current `connect-go` behavior discussed
in [connectrpc/connect-go#907](https://github.com/connectrpc/connect-go/issues/907).
connectrpc-axum prefers graceful degradation here so low `send_max_bytes` limits
do not turn streaming errors into empty success-looking HTTP 200 responses.

::: warning
By default, no limits are applied. For production environments, consider setting appropriate limits to protect against memory exhaustion attacks.
:::
