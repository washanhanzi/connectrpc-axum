# Connect RPC Client

The `connectrpc-axum-client` crate provides a Rust client for the Connect RPC protocol.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum-client = "0.1"
prost = "0.14"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
futures = "0.3"  # For streaming
```

For compression support:

```toml
[dependencies]
connectrpc-axum-client = { version = "0.1", features = ["compression-gzip-stream"] }
```

## Quick Start with Generated Client

The recommended way to use the client is with the generated typed client from `connectrpc-axum-build`:

```rust
use my_proto::hello_world_service_connect_client::HelloWorldServiceClient;

// Simple usage (panics on error)
let client = HelloWorldServiceClient::new("http://localhost:3000");

// With error handling
let client = HelloWorldServiceClient::builder("http://localhost:3000")
    .use_proto()
    .timeout(Duration::from_secs(30))
    .build()?;

// Make typed RPC calls
let response = client.say_hello(&HelloRequest {
    name: Some("Alice".to_string())
}).await?;

println!("Response: {:?}", response.into_inner());
```

### Generated Module Structure

For a service named `HelloWorldService`, the generated code creates:

```rust
// Procedure path constants (at root level)
pub mod hello_world_service_procedures {
    pub const SAY_HELLO: &str = "/hello.HelloWorldService/SayHello";
}

// Client module
pub mod hello_world_service_connect_client {
    pub struct HelloWorldServiceClient { ... }
    pub struct HelloWorldServiceClientBuilder { ... }
}
```

### new() vs builder()

Following the same pattern as `reqwest`:

- **`new(url)`** - Creates a client with default settings. Panics on error (e.g., TLS initialization failure).
- **`builder(url).build()?`** - Returns `Result`, allowing you to handle errors gracefully.

```rust
// Simple usage - panics on error
let client = HelloWorldServiceClient::new("http://localhost:3000");

// With error handling
let client = HelloWorldServiceClient::builder("http://localhost:3000")
    .use_proto()
    .build()?;
```

## Low-Level Client

For dynamic calls or when not using code generation, use `ConnectClient` directly:

```rust
use connectrpc_axum_client::ConnectClient;

// Create a client
let client = ConnectClient::builder("http://localhost:3000")
    .use_proto()  // Use protobuf encoding (default is JSON)
    .build()?;

// Make a unary call
let response = client.call_unary::<MyRequest, MyResponse>(
    "my.package.MyService/MyMethod",
    &request,
).await?;

println!("Response: {:?}", response.into_inner());
```

## Encoding

The client supports both JSON and Protobuf encoding:

```rust
// JSON encoding (default, broader compatibility)
let client = ConnectClient::builder("http://localhost:3000")
    .use_json()
    .build()?;

// Protobuf encoding (more efficient)
let client = ConnectClient::builder("http://localhost:3000")
    .use_proto()
    .build()?;
```

## Streaming RPCs

### Server Streaming

The server sends multiple messages in response to a single request:

```rust
use futures::StreamExt;

let response = client.call_server_stream::<ListRequest, ListItem>(
    "items.v1.ItemService/ListItems",
    &request,
).await?;

let mut stream = response.into_inner();
while let Some(result) = stream.next().await {
    match result {
        Ok(item) => println!("Got item: {:?}", item),
        Err(e) => eprintln!("Error: {:?}", e),
    }
}

// Access trailers after consuming the stream
if let Some(trailers) = stream.trailers() {
    println!("Trailers: {:?}", trailers);
}
```

### Client Streaming

The client sends multiple messages and receives a single response:

```rust
use futures::stream;

let messages = stream::iter(vec![
    Message { content: "first".to_string() },
    Message { content: "second".to_string() },
]);

let response = client.call_client_stream::<Message, Summary, _>(
    "chat.v1.ChatService/SendMessages",
    messages,
).await?;

println!("Summary: {:?}", response.into_inner());
```

### Bidirectional Streaming

Both client and server send streams of messages. Requires HTTP/2:

```rust
use futures::{stream, StreamExt};

let messages = stream::iter(vec![
    EchoRequest { message: "hello".to_string() },
    EchoRequest { message: "world".to_string() },
]);

let response = client.call_bidi_stream::<EchoRequest, EchoResponse, _>(
    "echo.v1.EchoService/EchoBidiStream",
    messages,
).await?;

let mut stream = response.into_inner();
while let Some(result) = stream.next().await {
    match result {
        Ok(msg) => println!("Got: {:?}", msg),
        Err(e) => eprintln!("Error: {:?}", e),
    }
}
```

## Per-Call Options

Customize individual calls with `CallOptions`:

```rust
use connectrpc_axum_client::CallOptions;
use std::time::Duration;

let options = CallOptions::new()
    .timeout(Duration::from_secs(5))
    .header("authorization", "Bearer token123")
    .header("x-request-id", "abc-123");

let response = client.call_unary_with_options::<Req, Res>(
    "service/Method",
    &request,
    options,
).await?;
```

## Timeouts

Configure timeouts at the client level or per-call:

```rust
use std::time::Duration;

// Client-wide default timeout
let client = ConnectClient::builder("http://localhost:3000")
    .timeout(Duration::from_secs(30))
    .build()?;

// Per-call timeout override
let options = CallOptions::new().timeout(Duration::from_secs(5));
let response = client.call_unary_with_options::<Req, Res>(
    "service/Method",
    &request,
    options,
).await?;
```

Timeouts are enforced on both client and server:

- **Client-side**: The request is cancelled if it exceeds the timeout
- **Server-side**: The `Connect-Timeout-Ms` header is sent, allowing cooperative cancellation

## Compression

Enable request compression:

```rust
use connectrpc_axum_client::{CompressionConfig, CompressionEncoding, CompressionLevel};

let client = ConnectClient::builder("http://localhost:3000")
    .compression(
        CompressionConfig::new(512)  // Compress bodies >= 512 bytes
            .level(CompressionLevel::Default)
    )
    .request_encoding(CompressionEncoding::Gzip)
    .accept_encoding(CompressionEncoding::Gzip)  // Accept compressed responses
    .build()?;
```

### Compression Feature Flags

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| `compression-gzip-stream` | Gzip compression | `flate2` |
| `compression-deflate-stream` | Deflate compression | `flate2` |
| `compression-br-stream` | Brotli compression | `brotli` |
| `compression-zstd-stream` | Zstandard compression | `zstd` |
| `compression-full-stream` | All compression algorithms | All of above |

## Retry Logic

Built-in retry support with exponential backoff following the gRPC connection backoff specification:

```rust
use connectrpc_axum_client::{retry, retry_with_policy, RetryPolicy};
use std::time::Duration;

// Using default retry policy (3 retries, 1s base delay)
let response = retry(|| async {
    client.call_unary::<Req, Res>("service/Method", &request).await
}).await?;

// Custom retry policy
let policy = RetryPolicy::new()
    .max_retries(5)
    .base_delay(Duration::from_millis(100))
    .max_delay(Duration::from_secs(10));

let response = retry_with_policy(&policy, || async {
    client.call_unary::<Req, Res>("service/Method", &request).await
}).await?;
```

### Retry Policy Presets

```rust
use connectrpc_axum_client::RetryPolicy;

// Default: 3 retries, 1s base delay, 120s max delay
let default = RetryPolicy::default();

// Aggressive: 5 retries, 50ms base delay, 1s max delay
// Good for latency-sensitive operations
let aggressive = RetryPolicy::aggressive();

// Patient: 10 retries, 2s base delay, 5 minute max delay
// Good for background jobs
let patient = RetryPolicy::patient();

// No retries
let no_retry = RetryPolicy::no_retry();
```

### Retryable Error Codes

Only certain errors trigger automatic retry:

- `Code::Unavailable` - Service temporarily unavailable
- `Code::ResourceExhausted` - Rate limited or quota exceeded
- `Code::Aborted` - Transaction aborted, can be retried
- Transport errors (connection failures, timeouts)

Non-retryable errors are returned immediately:

- `Code::InvalidArgument`
- `Code::NotFound`
- `Code::PermissionDenied`
- `Code::Unauthenticated`
- etc.

## Interceptors

Add cross-cutting logic to all RPC calls. The interceptor system provides two traits:

- **`Interceptor`** - Header-level access only (simple, no message bounds)
- **`MessageInterceptor`** - Full typed message access

### Header Interceptor

Add headers to all requests:

```rust
use connectrpc_axum_client::HeaderInterceptor;

let auth = HeaderInterceptor::new("authorization", "Bearer token123");

let client = ConnectClient::builder("http://localhost:3000")
    .with_interceptor(auth)
    .build()?;
```

### Closure Interceptor

Create quick header-level interceptors with closures:

```rust
use connectrpc_axum_client::{ClosureInterceptor, RequestContext};

let logging = ClosureInterceptor::new(|ctx: &mut RequestContext| {
    println!("Calling: {}", ctx.procedure);
    ctx.headers.insert("x-request-id", "abc-123".parse().unwrap());
    Ok(())
});

let client = ConnectClient::builder("http://localhost:3000")
    .with_interceptor(logging)
    .build()?;
```

### Custom Interceptor Trait

Implement the `Interceptor` trait for header-level cross-cutting concerns:

```rust
use connectrpc_axum_client::{Interceptor, RequestContext, ResponseContext, ClientError};

#[derive(Clone)]
struct AuthInterceptor {
    token: String,
}

impl Interceptor for AuthInterceptor {
    fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
        ctx.headers.insert("authorization", self.token.parse().unwrap());
        Ok(())
    }

    fn on_response(&self, ctx: &ResponseContext) -> Result<(), ClientError> {
        // Inspect response headers
        if let Some(value) = ctx.headers.get("x-ratelimit-remaining") {
            println!("Rate limit remaining: {:?}", value);
        }
        Ok(())
    }
}
```

### Message Interceptor

Implement `MessageInterceptor` for typed access to request/response messages:

```rust
use connectrpc_axum_client::{MessageInterceptor, RequestContext, ResponseContext, StreamContext, ClientError};
use prost::Message;

#[derive(Clone)]
struct LoggingInterceptor;

impl MessageInterceptor for LoggingInterceptor {
    fn on_request<Req>(
        &self,
        ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + serde::Serialize + 'static,
    {
        println!("Calling {} with {} bytes", ctx.procedure, request.encoded_len());
        Ok(())
    }

    fn on_response<Res>(
        &self,
        ctx: &ResponseContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + serde::de::DeserializeOwned + Default + 'static,
    {
        println!("Response from {} with {} bytes", ctx.procedure, response.encoded_len());
        Ok(())
    }

    fn on_stream_send<Req>(
        &self,
        ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + serde::Serialize + 'static,
    {
        println!("Streaming message to {}", ctx.procedure);
        Ok(())
    }

    fn on_stream_receive<Res>(
        &self,
        ctx: &StreamContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + serde::de::DeserializeOwned + Default + 'static,
    {
        println!("Received stream message from {}", ctx.procedure);
        Ok(())
    }
}

let client = ConnectClient::builder("http://localhost:3000")
    .with_message_interceptor(LoggingInterceptor)
    .build()?;
```

### Chaining Interceptors

Multiple interceptors can be chained. They execute in order for requests and reverse order for responses:

```rust
let client = ConnectClient::builder("http://localhost:3000")
    .with_interceptor(AuthInterceptor { token: "Bearer xyz".into() })
    .with_interceptor(HeaderInterceptor::new("x-api-version", "v1"))
    .with_message_interceptor(LoggingInterceptor)
    .build()?;
```

## Error Handling

The client returns `ClientError` for all failure cases:

```rust
use connectrpc_axum_client::{ClientError, Code};

match client.call_unary::<Req, Res>("service/Method", &request).await {
    Ok(response) => println!("Success: {:?}", response.into_inner()),
    Err(ClientError::Rpc(status)) => {
        println!("RPC error: {} - {:?}", status.code(), status.message());
        for detail in status.details() {
            println!("  Detail: {} = {:?}", detail.type_url(), detail.value());
        }
    }
    Err(ClientError::Transport(msg)) => {
        println!("Transport error: {}", msg);
    }
    Err(ClientError::Encode(msg)) => {
        println!("Encoding error: {}", msg);
    }
    Err(ClientError::Decode(msg)) => {
        println!("Decoding error: {}", msg);
    }
    Err(ClientError::Protocol(msg)) => {
        println!("Protocol error: {}", msg);
    }
}
```

### Error Code Mapping

| Variant | Code | Retryable |
|---------|------|-----------|
| `Rpc(status)` | From server | Depends on code |
| `Transport(_)` | `Unavailable` | Yes |
| `Encode(_)` | `Internal` | No |
| `Decode(_)` | `Internal` | No |
| `Protocol(_)` | `InvalidArgument` | No |

### Convenience Constructors

```rust
use connectrpc_axum_client::ClientError;

// Common error types
let err = ClientError::not_found("user not found");
let err = ClientError::invalid_argument("missing required field");
let err = ClientError::permission_denied("access denied");
let err = ClientError::unauthenticated("invalid token");
let err = ClientError::internal("unexpected error");
let err = ClientError::unavailable("service down");
```

## HTTP/2 Configuration

### HTTP/2 Prior Knowledge

For bidirectional streaming over unencrypted connections (h2c):

```rust
let client = ConnectClient::builder("http://localhost:3000")
    .http2_prior_knowledge()
    .build()?;
```

This is required for bidi streaming over `http://` URLs (e.g., development environments).

### Connection Pool

Configure connection pooling:

```rust
use std::time::Duration;

let client = ConnectClient::builder("http://localhost:3000")
    .pool_idle_timeout(Duration::from_secs(60))
    .build()?;
```

## TLS Configuration

### Custom Root Certificates

```rust
use connectrpc_axum_client::TlsClientConfig;
use std::sync::Arc;

// Load custom CA certificate
let mut roots = rustls::RootCertStore::empty();
roots.add_parsable_certificates(certs);

let tls_config = TlsClientConfig::builder()
    .with_root_certificates(roots)
    .with_no_client_auth();

let client = ConnectClient::builder("https://api.example.com")
    .tls_config(Arc::new(tls_config))
    .build()?;
```

### Disable Certificate Verification (Development Only)

```rust
// WARNING: Only use for development/testing!
let client = ConnectClient::builder("https://self-signed:3000")
    .danger_accept_invalid_certs()
    .build()?;
```

## Advanced Transport Configuration

For full control, create a custom transport:

```rust
use connectrpc_axum_client::{HyperTransportBuilder, ConnectClient};
use std::time::Duration;

let transport = HyperTransportBuilder::new()
    .http2_only(true)
    .pool_idle_timeout(Duration::from_secs(60))
    .build()?;

let client = ConnectClient::builder("http://localhost:3000")
    .with_transport(transport)
    .use_proto()
    .build()?;
```

## Response Metadata

Access response headers:

```rust
let response = client.call_unary::<Req, Res>("service/Method", &request).await?;

// Access metadata (headers)
if let Some(value) = response.metadata().get("x-custom-header") {
    println!("Custom header: {}", value);
}

// Extract the inner value
let inner = response.into_inner();
```

## Stream Cancellation

### Dropping the Stream

The simplest way to cancel - drop the stream:

```rust
let response = client.call_server_stream::<Req, Res>(...).await?;
let mut stream = response.into_inner();

// Process first few messages
for _ in 0..5 {
    if let Some(Ok(msg)) = stream.next().await {
        process(msg);
    }
}

// Dropping the stream cancels the RPC
drop(stream);
```

### Graceful Drain

For connection reuse, drain remaining messages:

```rust
let mut stream = response.into_inner();

// Process some messages
while let Some(Ok(msg)) = stream.next().await {
    if should_stop(&msg) {
        break;
    }
    process(msg);
}

// Gracefully drain remaining messages
stream.drain().await;

// Or with a timeout
stream.drain_timeout(Duration::from_secs(5)).await;
```

## Observability

Enable tracing with the `tracing` feature:

```toml
[dependencies]
connectrpc-axum-client = { version = "0.1", features = ["tracing"] }
```

Each RPC call creates a span with:

- `rpc.method`: Full procedure name (e.g., "package.Service/Method")
- `rpc.type`: Call type ("unary", "server_stream", "client_stream", "bidi_stream")
- `rpc.encoding`: Message encoding ("json" or "proto")
- `otel.kind`: "client"

## Feature Flags Summary

| Feature | Description |
|---------|-------------|
| `compression-gzip-stream` | Gzip compression for streaming |
| `compression-deflate-stream` | Deflate compression |
| `compression-br-stream` | Brotli compression |
| `compression-zstd-stream` | Zstandard compression |
| `compression-full-stream` | All compression algorithms |
| `tracing` | OpenTelemetry-compatible tracing |

## Example: Complete Setup

```rust
use connectrpc_axum_client::{
    ConnectClient, CallOptions, HeaderInterceptor, RetryPolicy,
    CompressionConfig, CompressionEncoding, CompressionLevel,
    retry_with_policy,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client with full configuration
    let client = ConnectClient::builder("https://api.example.com")
        .use_proto()
        .timeout(Duration::from_secs(30))
        .compression(CompressionConfig::new(512).level(CompressionLevel::Default))
        .request_encoding(CompressionEncoding::Gzip)
        .accept_encoding(CompressionEncoding::Gzip)
        .with_interceptor(HeaderInterceptor::new("authorization", "Bearer token"))
        .build()?;

    // Make a call with retry
    let policy = RetryPolicy::default();
    let response = retry_with_policy(&policy, || async {
        client.call_unary::<MyRequest, MyResponse>(
            "my.package.MyService/MyMethod",
            &MyRequest { id: "123".into() },
        ).await
    }).await?;

    println!("Response: {:?}", response.into_inner());
    Ok(())
}
```
