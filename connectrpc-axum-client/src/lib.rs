//! Connect protocol client for Rust.
//!
//! This crate provides a client implementation for the Connect RPC protocol,
//! designed to work with servers built using `connectrpc-axum`.
//!
//! ## Features
//!
//! - Unary RPC calls (request-response)
//! - Server streaming RPC calls
//! - Client streaming RPC calls
//! - Bidirectional streaming RPC calls (requires HTTP/2)
//! - Both protobuf and JSON encoding support
//! - Request compression (gzip, brotli, zstd)
//! - Response decompression
//!
//! ## Example
//!
//! ```ignore
//! use connectrpc_axum_client::ConnectClient;
//!
//! // Create a client
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .use_proto()
//!     .build()?;
//!
//! // Make a unary call
//! let response = client.call_unary::<MyRequest, MyResponse>(
//!     "my.package.MyService/MyMethod",
//!     &request,
//! ).await?;
//!
//! println!("Response: {:?}", response.into_inner());
//! ```
//!
//! ## Server Streaming Example
//!
//! ```ignore
//! use connectrpc_axum_client::ConnectClient;
//! use futures::StreamExt;
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .use_proto()
//!     .build()?;
//!
//! // Make a server streaming call
//! let response = client.call_server_stream::<ListRequest, ListItem>(
//!     "items.v1.ItemService/ListItems",
//!     &request,
//! ).await?;
//!
//! let mut stream = response.into_inner();
//! while let Some(result) = stream.next().await {
//!     match result {
//!         Ok(item) => println!("Got item: {:?}", item),
//!         Err(e) => eprintln!("Error: {:?}", e),
//!     }
//! }
//!
//! // Access trailers after consuming the stream
//! if let Some(trailers) = stream.trailers() {
//!     println!("Trailers: {:?}", trailers);
//! }
//! ```
//!
//! ## Bidirectional Streaming Example
//!
//! ```ignore
//! use connectrpc_axum_client::ConnectClient;
//! use futures::{stream, StreamExt};
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .use_proto()
//!     .build()?;
//!
//! // Create a stream of request messages
//! let messages = stream::iter(vec![
//!     EchoRequest { message: "hello".to_string() },
//!     EchoRequest { message: "world".to_string() },
//! ]);
//!
//! // Make a bidi streaming call (requires HTTP/2)
//! let response = client.call_bidi_stream::<EchoRequest, EchoResponse, _>(
//!     "echo.v1.EchoService/EchoBidiStream",
//!     messages,
//! ).await?;
//!
//! let mut stream = response.into_inner();
//! while let Some(result) = stream.next().await {
//!     match result {
//!         Ok(msg) => println!("Got: {:?}", msg),
//!         Err(e) => eprintln!("Error: {:?}", e),
//!     }
//! }
//! ```
//!
//! ## Streaming Cancellation
//!
//! Streaming RPCs can be cancelled in several ways:
//!
//! ### Dropping the Stream
//!
//! The simplest way to cancel a stream is to drop it. When a [`Streaming`] is
//! dropped, the underlying HTTP connection is closed, which signals cancellation
//! to the server via TCP RST or HTTP/2 RST_STREAM.
//!
//! ```ignore
//! let response = client.call_server_stream::<Req, Res>(...).await?;
//! let mut stream = response.into_inner();
//!
//! // Process first few messages
//! for _ in 0..5 {
//!     if let Some(Ok(msg)) = stream.next().await {
//!         process(msg);
//!     }
//! }
//!
//! // Dropping the stream here cancels the RPC
//! drop(stream);
//! ```
//!
//! ### Using tokio::select!
//!
//! Use `tokio::select!` to race a stream against a cancellation signal:
//!
//! ```ignore
//! use tokio::sync::oneshot;
//!
//! let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
//!
//! let response = client.call_server_stream::<Req, Res>(...).await?;
//! let mut stream = response.into_inner();
//!
//! loop {
//!     tokio::select! {
//!         _ = cancel_rx => {
//!             // Cancellation requested - stream is dropped here
//!             break;
//!         }
//!         item = stream.next() => {
//!             match item {
//!                 Some(Ok(msg)) => process(msg),
//!                 Some(Err(e)) => handle_error(e),
//!                 None => break, // Stream completed
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ### Timeouts
//!
//! Set timeouts using [`ClientBuilder::timeout`] or [`CallOptions::timeout`].
//! The timeout is propagated to the server via the `Connect-Timeout-Ms` header,
//! allowing cooperative cancellation.
//!
//! ```ignore
//! use std::time::Duration;
//!
//! // Client-wide timeout
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .timeout(Duration::from_secs(30))
//!     .build()?;
//!
//! // Per-call timeout override
//! let options = CallOptions::new().timeout(Duration::from_secs(5));
//! let response = client.call_server_stream_with_options::<Req, Res>(
//!     "service/Method",
//!     &request,
//!     options,
//! ).await?;
//! ```
//!
//! ### Error Codes
//!
//! - [`Code::Canceled`] - The RPC was cancelled (client dropped the stream)
//! - [`Code::DeadlineExceeded`] - The timeout was exceeded
//!
//! When a stream is cancelled, the server may receive an error or simply see
//! the connection close. Well-behaved servers should clean up resources when
//! this happens.
//!
//! ### Graceful Shutdown
//!
//! For graceful stream shutdown that allows connection reuse, use the
//! [`drain()`](Streaming::drain) or [`drain_timeout()`](Streaming::drain_timeout)
//! methods instead of dropping the stream:
//!
//! ```ignore
//! let mut stream = response.into_inner();
//!
//! // Process messages until done
//! while let Some(Ok(msg)) = stream.next().await {
//!     if should_stop(&msg) {
//!         break;
//!     }
//!     process(msg);
//! }
//!
//! // Gracefully drain remaining messages (enables connection reuse)
//! stream.drain().await;
//!
//! // Or with a timeout to prevent hanging
//! stream.drain_timeout(Duration::from_secs(5)).await;
//! ```
//!
//! ## Feature Flags
//!
//! JSON and Protobuf encoding are always available. The default feature set
//! enables TLS with `ring` and native root certificates, with no compression or
//! tracing.
//!
//! ### TLS
//!
//! | Feature | Description | Dependencies |
//! |---------|-------------|--------------|
//! | `tls` | Default TLS setup | `tls-ring`, `tls-native-roots` |
//! | `tls-ring` | Use the ring crypto provider | `rustls`, `hyper-rustls` |
//! | `tls-aws-lc` | Use the AWS-LC crypto provider | `rustls`, `hyper-rustls` |
//! | `tls-native-roots` | Use system root certificates | `rustls-native-certs` |
//! | `tls-webpki-roots` | Use bundled Mozilla root certificates | `webpki-roots` |
//!
//! ### Compression
//!
//! | Feature | Description | Dependencies |
//! |---------|-------------|--------------|
//! | `compression-gzip-stream` | Gzip compression (common) | `flate2` |
//! | `compression-deflate-stream` | Deflate compression | `flate2` |
//! | `compression-br-stream` | Brotli compression (high ratio) | `brotli` |
//! | `compression-zstd-stream` | Zstandard compression (fast) | `zstd` |
//! | `compression-full-stream` | All compression algorithms | All of above |
//!
//! **Recommendation**: Use `compression-gzip-stream` for best compatibility. The
//! server and client negotiate compression automatically via `Accept-Encoding`
//! headers.
//!
//! ### Observability
//!
//! | Feature | Description | Dependencies |
//! |---------|-------------|--------------|
//! | `tracing` | Tracing spans for RPC calls | `tracing` |
//!
//! When enabled, each RPC call creates a span with:
//! - `rpc.method`: Full procedure name (e.g., "package.Service/Method")
//! - `rpc.type`: Call type ("unary", "server_stream", "client_stream", "bidi_stream")
//! - `rpc.encoding`: Message encoding ("json" or "proto")
//! - `otel.kind`: "client"
//!
//! ### Example: Common Configurations
//!
//! ```toml
//! # Minimal (no compression, no tracing)
//! connectrpc-axum-client = "0.1"
//!
//! # With gzip compression
//! connectrpc-axum-client = { version = "0.1", features = ["compression-gzip-stream"] }
//!
//! # Full observability setup
//! connectrpc-axum-client = { version = "0.1", features = ["tracing", "compression-gzip-stream"] }
//!
//! # Maximum compatibility
//! connectrpc-axum-client = { version = "0.1", features = ["compression-full-stream", "tracing"] }
//! ```
//!
//! ## Retry Logic
//!
//! The client provides built-in retry support with exponential backoff and jitter,
//! following the [gRPC connection backoff specification](https://github.com/grpc/grpc/blob/master/doc/connection-backoff.md).
//!
//! ### Using the Retry Helpers
//!
//! The simplest way to add retries is with the [`retry`] or [`retry_with_policy`] functions:
//!
//! ```ignore
//! use connectrpc_axum_client::{ConnectClient, retry, retry_with_policy, RetryPolicy};
//! use std::time::Duration;
//!
//! // Using default retry policy (3 retries, 1s base delay)
//! let response = retry(|| async {
//!     client.call_unary::<MyRequest, MyResponse>("service/Method", &request).await
//! }).await?;
//!
//! // Using custom retry policy
//! let policy = RetryPolicy::new()
//!     .max_retries(5)
//!     .base_delay(Duration::from_millis(100))
//!     .max_delay(Duration::from_secs(10));
//!
//! let response = retry_with_policy(&policy, || async {
//!     client.call_unary::<MyRequest, MyResponse>("service/Method", &request).await
//! }).await?;
//! ```
//!
//! ### Retry Policy Presets
//!
//! Several preset policies are available for common use cases:
//!
//! ```ignore
//! use connectrpc_axum_client::RetryPolicy;
//!
//! // Default: 3 retries, 1s base delay, 120s max delay
//! let default = RetryPolicy::default();
//!
//! // Aggressive: 5 retries, 50ms base delay, 1s max delay
//! // Good for latency-sensitive operations
//! let aggressive = RetryPolicy::aggressive();
//!
//! // Patient: 10 retries, 2s base delay, 5 minute max delay
//! // Good for background jobs
//! let patient = RetryPolicy::patient();
//!
//! // No retries (useful for testing or disabling)
//! let no_retry = RetryPolicy::no_retry();
//! ```
//!
//! ### Exponential Backoff
//!
//! The backoff algorithm uses:
//! - **Base delay**: Initial wait time before first retry (default: 1s)
//! - **Multiplier**: Factor to increase delay each retry (default: 1.6)
//! - **Jitter**: Random variation to prevent thundering herd (default: 20%)
//! - **Max delay**: Upper bound on wait time (default: 120s)
//!
//! Example sequence with default settings (no jitter for clarity):
//! - Retry 1: ~1.0s
//! - Retry 2: ~1.6s
//! - Retry 3: ~2.6s
//!
//! ### Retryable Error Codes
//!
//! Only certain errors are automatically retried:
//! - [`Code::Unavailable`] - Service temporarily unavailable
//! - [`Code::ResourceExhausted`] - Rate limited or quota exceeded
//! - [`Code::Aborted`] - Transaction aborted, can be retried
//! - Transport errors (connection failures, timeouts)
//!
//! Non-retryable errors (e.g., `InvalidArgument`, `NotFound`, `PermissionDenied`)
//! are returned immediately without retry.
//!
//! ### Manual Retry Control
//!
//! For more control, use [`ExponentialBackoff`] directly:
//!
//! ```ignore
//! use connectrpc_axum_client::{RetryPolicy, ExponentialBackoff, ClientError};
//!
//! async fn custom_retry<T, F, Fut>(mut operation: F) -> Result<T, ClientError>
//! where
//!     F: FnMut() -> Fut,
//!     Fut: std::future::Future<Output = Result<T, ClientError>>,
//! {
//!     let policy = RetryPolicy::new().max_retries(3);
//!     let mut backoff = policy.backoff();
//!
//!     loop {
//!         match operation().await {
//!             Ok(result) => return Ok(result),
//!             Err(e) if e.is_retryable() && backoff.can_retry() => {
//!                 let delay = backoff.next_delay();
//!                 println!("Retry {} in {:?}", backoff.attempts(), delay);
//!                 tokio::time::sleep(delay).await;
//!             }
//!             Err(e) => return Err(e),
//!         }
//!     }
//! }
//! ```
//!
//! ### Checking Retryability
//!
//! Use [`ClientError::is_retryable()`] or [`Code::is_retryable()`] to check if
//! an error should be retried:
//!
//! ```ignore
//! use connectrpc_axum_client::{ClientError, Code};
//!
//! let err = ClientError::unavailable("service overloaded");
//! assert!(err.is_retryable());
//!
//! let err = ClientError::not_found("user not found");
//! assert!(!err.is_retryable());
//! ```
//!
//! ## Per-Call Options
//!
//! For per-call customization, use [`CallOptions`]:
//!
//! ```ignore
//! use connectrpc_axum_client::CallOptions;
//!
//! let options = CallOptions::new()
//!     .header("X-Custom-Header", "value")
//!     .timeout(std::time::Duration::from_secs(5));
//!
//! let response = client.call_unary_with_options::<Req, Res>(
//!     "service/Method",
//!     &request,
//!     options,
//! ).await?;
//! ```
//!
//! ## Testing and Protocol Conformance
//!
//! This client is tested for Connect protocol conformance at multiple levels:
//!
//! ### Unit Tests
//!
//! Internal components are tested via `#[cfg(test)]` modules:
//! - Frame encoding/decoding ([`FrameEncoder`], [`FrameDecoder`])
//! - Error parsing and code mapping
//! - Compression handling
//! - Response wrapper methods
//!
//! Run with: `cargo test -p connectrpc-axum-client`
//!
//! ### Integration Tests
//!
//! The `connectrpc-axum-examples` crate provides integration tests:
//!
//! **Rust client tests** (`src/bin/client/`):
//! - `unary-client`: Tests JSON/Proto encoding, error handling, response wrappers
//! - `server-stream-client`: Tests server streaming with trailers
//! - `client-stream-client`: Tests client streaming
//! - `bidi-stream-client`: Tests bidirectional streaming
//!
//! **Go client conformance tests** (`go-client/`):
//! - Protocol version validation
//! - Timeout handling (`Connect-Timeout-Ms`)
//! - Compression algorithms
//! - Error details and metadata
//! - Streaming error handling
//! - GET request support (idempotent RPCs)
//!
//! Run with: `go test -C connectrpc-axum-examples/go-client -v`
//!
//! ### Wire Format
//!
//! The client implements the [Connect Protocol Specification]:
//! - **Unary**: Standard HTTP POST with JSON or Protobuf body
//! - **Streaming**: Enveloped messages with 5-byte header `[flags:1][length:4]`
//! - **End Stream**: Flags=0x02, contains JSON with optional error/metadata
//! - **Errors**: JSON body with `code`, `message`, and optional `details`
//!
//! [Connect Protocol Specification]: https://connectrpc.com/docs/protocol
//!
//! ## TLS Configuration
//!
//! The client uses [rustls](https://docs.rs/rustls) for TLS by default through
//! [`HyperTransport`]. Use [`ClientBuilder::tls_config`] for custom TLS settings,
//! or pass a pre-configured [`HyperTransport`] with [`ClientBuilder::with_transport`].
//! Add `rustls` as a direct dependency when constructing a custom
//! [`TlsClientConfig`].
//!
//! ### Custom Root Certificates
//!
//! ```ignore
//! use connectrpc_axum_client::{ConnectClient, TlsClientConfig};
//! use std::fs;
//!
//! // Load a DER-encoded custom CA certificate.
//! let cert = rustls::pki_types::CertificateDer::from(fs::read("ca.der")?);
//! let mut roots = rustls::RootCertStore::empty();
//! roots.add(cert)?;
//!
//! let tls_config = TlsClientConfig::builder()
//!     .with_root_certificates(roots)
//!     .with_no_client_auth();
//!
//! let client = ConnectClient::builder("https://internal-service:3000")
//!     .tls_config(tls_config)
//!     .build()?;
//! ```
//!
//! ### Client Certificates (mTLS)
//!
//! ```ignore
//! use connectrpc_axum_client::{ConnectClient, TlsClientConfig};
//! use rustls::pki_types::{CertificateDer, PrivateKeyDer};
//!
//! let roots = rustls::RootCertStore::empty();
//! let cert_chain: Vec<CertificateDer<'static>> = load_client_cert_chain()?;
//! let private_key: PrivateKeyDer<'static> = load_client_private_key()?;
//!
//! let tls_config = TlsClientConfig::builder()
//!     .with_root_certificates(roots)
//!     .with_client_auth_cert(cert_chain, private_key)?;
//!
//! let client = ConnectClient::builder("https://mtls-server:3000")
//!     .tls_config(tls_config)
//!     .build()?;
//! ```
//!
//! ### Disable Certificate Verification (Development Only)
//!
//! ```ignore
//! use connectrpc_axum_client::ConnectClient;
//!
//! // WARNING: Only use for development/testing!
//! let client = ConnectClient::builder("https://self-signed:3000")
//!     .danger_accept_invalid_certs()
//!     .build()?;
//! ```
//!
//! ## Advanced Transport Configuration
//!
//! For full control over HTTP/2 and connection pooling, create a custom
//! [`HyperTransport`]:
//!
//! ```ignore
//! use connectrpc_axum_client::{ConnectClient, HyperTransportBuilder};
//! use std::time::Duration;
//!
//! let transport = HyperTransportBuilder::new()
//!     .http2_only(true)
//!     .pool_idle_timeout(Duration::from_secs(60))
//!     .build()?;
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .with_transport(transport)
//!     .use_proto()
//!     .build()?;
//! ```
//!
//! ## WASM Compatibility
//!
//! **Note:** This client does not currently support WebAssembly (wasm32) targets.
//!
//! ### Current Limitations
//!
//! The client relies on Hyper/Tokio features that are not available in WASM:
//!
//! 1. **Streaming request bodies**: Client and bidirectional streaming use
//!    Hyper request bodies backed by Rust streams, not browser `fetch` bodies.
//!
//! 2. **HTTP/2 prior knowledge**: Browser environments handle protocol
//!    negotiation and do not expose h2c prior knowledge.
//!
//! 3. **Connection pooling and keep-alive**: Low-level socket options are not
//!    exposed in browser APIs.
//!
//! ### Unary Calls in WASM
//!
//! For WASM targets that only need unary RPC calls, a future version may provide
//! conditional compilation to exclude streaming methods. Contributions welcome.
//!
//! ### Alternatives for WASM
//!
//! For browser-based Connect clients, consider:
//! - [Connect-ES](https://connectrpc.com/docs/web/getting-started): Official
//!   TypeScript/JavaScript client for browsers
//! - [connect-query](https://connectrpc.com/docs/web/connect-query): TanStack
//!   Query integration for React/Vue/Svelte

mod builder;
mod client;
pub mod config;
mod error;
pub mod request;
pub mod response;
pub mod transport;

pub use builder::{ClientBuildError, ClientBuilder};
pub use client::{ConnectClient, RawStreamingResponse};
pub use error::ClientError;

// Re-export from config module
pub use config::{
    BidiStreamInterceptors, CallOptions, Chain, ClientStreamInterceptors, ClosureInterceptor,
    ExponentialBackoff, HeaderInterceptor, HeaderWrapper, Interceptor, InterceptorInternal,
    MessageInterceptor, MessageWrapper, RequestContext, ResponseContext, RetryPolicy,
    ServerStreamInterceptors, StreamContext, StreamType, TypedInterceptor, TypedMutInterceptor,
    UnaryInterceptors, response_interceptor, retry, retry_with_policy, stream_interceptor,
};

// Re-export from request module
pub use request::FrameEncoder;

// Re-export from response module
#[allow(deprecated)]
pub use response::InterceptingStream;
pub use response::{
    ConnectResponse, FrameDecoder, InterceptingSendStream, InterceptingStreaming, Metadata,
    SendInterceptorError, Streaming, TypedReceiveStreaming, TypedSendStream,
};

// Re-export transport types at the top level for convenience
pub use transport::{HyperTransport, HyperTransportBuilder, TlsClientConfig, TransportBody};

// Re-export core types that users need
pub use connectrpc_axum_core::{
    Code, CompressionConfig, CompressionEncoding, CompressionLevel, ErrorDetail, Status,
};

// Re-export types needed for generated streaming code
pub use bytes::Bytes;
pub use http::HeaderMap;
