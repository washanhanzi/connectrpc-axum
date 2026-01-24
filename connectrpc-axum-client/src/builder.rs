//! Client builder for Connect RPC client.
//!
//! Provides a fluent API for configuring and building a [`ConnectClient`].

use crate::client::ConnectClient;
use crate::config::{Interceptor, InterceptorChain};
use crate::transport::{HyperTransport, HyperTransportBuilder, TlsClientConfig};
use connectrpc_axum_core::{CompressionConfig, CompressionEncoding};
use std::sync::Arc;
use std::time::Duration;

/// Builder for creating a [`ConnectClient`].
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{ClientBuilder, CompressionEncoding};
///
/// let client = ClientBuilder::new("http://localhost:3000")
///     .use_proto()  // Use protobuf encoding (default is JSON)
///     .accept_encoding(CompressionEncoding::Gzip)
///     .build()?;
/// ```
pub struct ClientBuilder {
    /// Base URL for the service (e.g., "http://localhost:3000").
    base_url: String,
    /// Optional pre-configured transport.
    transport: Option<HyperTransport>,
    /// Transport builder for when transport is not directly provided.
    transport_builder: HyperTransportBuilder,
    /// Use protobuf encoding (true) or JSON encoding (false).
    use_proto: bool,
    /// Compression configuration for outgoing requests.
    compression: Option<CompressionConfig>,
    /// Compression encoding for outgoing request bodies.
    request_encoding: CompressionEncoding,
    /// Accepted compression encodings for responses.
    accept_encoding: Option<CompressionEncoding>,
    /// Default timeout for RPC calls.
    default_timeout: Option<Duration>,
    /// Interceptor chain for RPC calls.
    interceptors: InterceptorChain,
}

impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("base_url", &self.base_url)
            .field("transport", &self.transport.is_some())
            .field("use_proto", &self.use_proto)
            .field("compression", &self.compression)
            .field("request_encoding", &self.request_encoding)
            .field("accept_encoding", &self.accept_encoding)
            .field("default_timeout", &self.default_timeout)
            .field("interceptors", &self.interceptors.len())
            .finish()
    }
}

impl ClientBuilder {
    /// Create a new ClientBuilder with the given base URL.
    ///
    /// The base URL should include the scheme and host, e.g., "http://localhost:3000".
    /// Do not include a trailing slash.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let builder = ClientBuilder::new("http://localhost:3000");
    /// ```
    pub fn new<S: Into<String>>(base_url: S) -> Self {
        Self {
            base_url: base_url.into(),
            transport: None,
            transport_builder: HyperTransportBuilder::new(),
            use_proto: false, // Default to JSON for broader compatibility
            compression: None,
            request_encoding: CompressionEncoding::Identity,
            accept_encoding: None,
            default_timeout: None,
            interceptors: InterceptorChain::new(),
        }
    }

    /// Use a pre-configured HyperTransport.
    ///
    /// This allows you to configure TLS, HTTP/2, connection pooling, etc.
    /// on the underlying transport.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::{HyperTransportBuilder, ClientBuilder};
    /// use std::time::Duration;
    ///
    /// let transport = HyperTransportBuilder::new()
    ///     .http2_only(true)
    ///     .pool_idle_timeout(Duration::from_secs(60))
    ///     .build()?;
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .with_transport(transport)
    ///     .build()?;
    /// ```
    pub fn with_transport(mut self, transport: HyperTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Use JSON encoding for requests and responses.
    ///
    /// This is the default encoding.
    pub fn use_json(mut self) -> Self {
        self.use_proto = false;
        self
    }

    /// Use protobuf encoding for requests and responses.
    ///
    /// Protobuf is more efficient than JSON but requires the server
    /// to support the `application/proto` content type.
    pub fn use_proto(mut self) -> Self {
        self.use_proto = true;
        self
    }

    /// Configure compression for outgoing requests.
    ///
    /// # Arguments
    ///
    /// * `config` - Compression configuration (threshold, level)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::{CompressionConfig, CompressionLevel};
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .compression(CompressionConfig::new(1024).level(CompressionLevel::Fastest))
    ///     .request_encoding(CompressionEncoding::Gzip)
    ///     .build()?;
    /// ```
    pub fn compression(mut self, config: CompressionConfig) -> Self {
        self.compression = Some(config);
        self
    }

    /// Set the compression encoding for outgoing request bodies.
    ///
    /// Default is `Identity` (no compression).
    ///
    /// Note: You should also call `compression()` to configure when
    /// compression is applied (min bytes threshold, level).
    pub fn request_encoding(mut self, encoding: CompressionEncoding) -> Self {
        self.request_encoding = encoding;
        self
    }

    /// Set the accepted compression encoding for responses.
    ///
    /// This sets the `Accept-Encoding` header on requests, telling
    /// the server what compression algorithms the client supports.
    ///
    /// If not set, no `Accept-Encoding` header is sent (server chooses).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .accept_encoding(CompressionEncoding::Gzip)
    ///     .build()?;
    /// ```
    pub fn accept_encoding(mut self, encoding: CompressionEncoding) -> Self {
        self.accept_encoding = Some(encoding);
        self
    }

    /// Set the default timeout for RPC calls.
    ///
    /// This timeout is enforced on both client and server sides:
    /// - **Client-side**: The request will be cancelled if it exceeds the timeout
    /// - **Server-side**: The `Connect-Timeout-Ms` header is sent, allowing the
    ///   server to cancel processing if the deadline will be exceeded
    ///
    /// For unary and client-streaming RPCs, the timeout applies to the entire
    /// call including connection, request, and response.
    ///
    /// For server-streaming and bidirectional RPCs, the timeout applies to the
    /// initial connection and response establishment. Stream consumption is not
    /// subject to this timeout (use [`Streaming::drain_timeout`] for that).
    ///
    /// Individual calls can override this timeout using [`CallOptions::timeout`].
    ///
    /// The maximum supported timeout is approximately 115 days (10 digit milliseconds).
    /// Larger values will be treated as no timeout.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .timeout(Duration::from_secs(30))
    ///     .build()?;
    /// ```
    ///
    /// [`CallOptions::timeout`]: crate::CallOptions::timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    /// Add an interceptor to the client.
    ///
    /// Interceptors allow you to add cross-cutting logic to RPC calls, such as:
    /// - Adding authentication headers
    /// - Logging and metrics
    /// - Retry logic
    /// - Request/response transformation
    ///
    /// Interceptors are applied in the order they are added. The first interceptor
    /// added is the first to process outgoing requests and the last to process
    /// incoming responses.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::{ClientBuilder, HeaderInterceptor};
    ///
    /// // Add an auth header to all requests
    /// let auth_interceptor = HeaderInterceptor::new("authorization", "Bearer token123");
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .with_interceptor(auth_interceptor)
    ///     .build()?;
    /// ```
    pub fn with_interceptor<I: Interceptor + 'static>(mut self, interceptor: I) -> Self {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    /// Enable HTTP/2 prior knowledge (h2c) for unencrypted connections.
    ///
    /// When enabled, the client will use HTTP/2 directly without the HTTP/1.1
    /// upgrade handshake. This is required for bidirectional streaming over
    /// unencrypted connections (e.g., `http://` URLs in development).
    ///
    /// **When to use:**
    /// - Development environments without TLS
    /// - Internal services behind a load balancer that terminates TLS
    /// - Any scenario where you need bidi streaming over `http://`
    ///
    /// **Note:** This setting only applies when the builder creates the transport.
    /// If you provide your own transport via [`with_transport()`], configure HTTP/2
    /// on that transport's builder instead.
    ///
    /// For HTTPS connections, HTTP/2 is negotiated via ALPN automatically,
    /// so this setting is not needed.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // For development with bidi streaming over http://
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .http2_prior_knowledge()
    ///     .use_proto()
    ///     .build()?;
    /// ```
    ///
    /// [`with_transport()`]: Self::with_transport
    pub fn http2_prior_knowledge(mut self) -> Self {
        self.transport_builder = self.transport_builder.http2_only(true);
        self
    }

    /// Set the connection pool idle timeout.
    ///
    /// Connections that have been idle for longer than this duration
    /// will be closed and removed from the pool.
    ///
    /// **Note:** This setting only applies when the builder creates the transport.
    /// If you provide your own transport via [`with_transport()`], configure this
    /// on that transport's builder instead.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .pool_idle_timeout(Duration::from_secs(60))
    ///     .build()?;
    /// ```
    ///
    /// [`with_transport()`]: Self::with_transport
    pub fn pool_idle_timeout(mut self, timeout: Duration) -> Self {
        self.transport_builder = self.transport_builder.pool_idle_timeout(timeout);
        self
    }

    /// Set a custom TLS configuration.
    ///
    /// Use this to configure custom root certificates, client certificates for mTLS,
    /// or other TLS settings.
    ///
    /// **Note:** This setting only applies when the builder creates the transport.
    /// If you provide your own transport via [`with_transport()`], configure TLS
    /// on that transport's builder instead.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::TlsClientConfig;
    ///
    /// let tls_config = TlsClientConfig::builder()
    ///     .with_root_certificates(my_roots)
    ///     .with_no_client_auth();
    ///
    /// let client = ClientBuilder::new("https://api.example.com")
    ///     .tls_config(tls_config)
    ///     .build()?;
    /// ```
    ///
    /// [`with_transport()`]: Self::with_transport
    pub fn tls_config(mut self, config: TlsClientConfig) -> Self {
        self.transport_builder = self.transport_builder.tls_config(config);
        self
    }

    /// Accept invalid TLS certificates.
    ///
    /// # Warning
    ///
    /// This is extremely dangerous and should only be used for development/testing!
    /// It makes the connection vulnerable to man-in-the-middle attacks.
    ///
    /// **Note:** This setting only applies when the builder creates the transport.
    /// If you provide your own transport via [`with_transport()`], configure this
    /// on that transport's builder instead.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // WARNING: Only use for development/testing!
    /// let client = ClientBuilder::new("https://self-signed:3000")
    ///     .danger_accept_invalid_certs()
    ///     .build()?;
    /// ```
    ///
    /// [`with_transport()`]: Self::with_transport
    pub fn danger_accept_invalid_certs(mut self) -> Self {
        self.transport_builder = self.transport_builder.danger_accept_invalid_certs();
        self
    }

    /// Build the ConnectClient.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP transport cannot be created.
    pub fn build(self) -> Result<ConnectClient, ClientBuildError> {
        // Create or use provided transport
        let transport = match self.transport {
            Some(t) => t,
            None => self
                .transport_builder
                .build()
                .map_err(|e| ClientBuildError::Transport(e.to_string()))?,
        };

        // Normalize base URL (remove trailing slash)
        let base_url = self.base_url.trim_end_matches('/').to_string();

        Ok(ConnectClient::new(
            transport,
            base_url,
            self.use_proto,
            self.compression.unwrap_or_default(),
            self.request_encoding,
            self.accept_encoding,
            self.default_timeout,
            self.interceptors,
        ))
    }
}

/// Error type for client building failures.
#[derive(Debug, thiserror::Error)]
pub enum ClientBuildError {
    /// Failed to create HTTP transport.
    #[error("failed to create HTTP transport: {0}")]
    Transport(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = ClientBuilder::new("http://localhost:3000");
        assert!(!builder.use_proto);
        assert!(builder.transport.is_none());
    }

    #[test]
    fn test_builder_use_proto() {
        let builder = ClientBuilder::new("http://localhost:3000").use_proto();
        assert!(builder.use_proto);
    }

    #[test]
    fn test_builder_use_json() {
        let builder = ClientBuilder::new("http://localhost:3000")
            .use_proto()
            .use_json(); // Switch back
        assert!(!builder.use_proto);
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_builder_accept_encoding() {
        let builder =
            ClientBuilder::new("http://localhost:3000").accept_encoding(CompressionEncoding::Gzip);
        assert_eq!(builder.accept_encoding, Some(CompressionEncoding::Gzip));
    }

    #[cfg(feature = "compression-gzip-stream")]
    #[test]
    fn test_builder_compression() {
        let config = CompressionConfig::new(512);
        let builder = ClientBuilder::new("http://localhost:3000")
            .compression(config)
            .request_encoding(CompressionEncoding::Gzip);
        assert!(builder.compression.is_some());
        assert_eq!(builder.compression.unwrap().min_bytes, 512);
        assert_eq!(builder.request_encoding, CompressionEncoding::Gzip);
    }

    #[test]
    fn test_builder_build() {
        let result = ClientBuilder::new("http://localhost:3000").build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_normalizes_url() {
        let client = ClientBuilder::new("http://localhost:3000/")
            .build()
            .unwrap();
        // The trailing slash should be removed
        assert!(!client.base_url().ends_with('/'));
    }

    #[test]
    fn test_builder_timeout() {
        let builder = ClientBuilder::new("http://localhost:3000").timeout(Duration::from_secs(30));
        assert_eq!(builder.default_timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_builder_timeout_default_none() {
        let builder = ClientBuilder::new("http://localhost:3000");
        assert!(builder.default_timeout.is_none());
    }

    #[test]
    fn test_builder_http2_prior_knowledge() {
        // Just verify it builds successfully with http2_prior_knowledge enabled
        let result = ClientBuilder::new("http://localhost:3000")
            .http2_prior_knowledge()
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_pool_idle_timeout() {
        // Verify it builds successfully with pool_idle_timeout set
        let result = ClientBuilder::new("http://localhost:3000")
            .pool_idle_timeout(Duration::from_secs(60))
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_combined_transport_options() {
        // Verify that multiple transport options can be combined
        let result = ClientBuilder::new("http://localhost:3000")
            .http2_prior_knowledge()
            .pool_idle_timeout(Duration::from_secs(60))
            .timeout(Duration::from_secs(30))
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_with_custom_transport() {
        let transport = HyperTransportBuilder::new()
            .http2_only(true)
            .build()
            .unwrap();

        let result = ClientBuilder::new("http://localhost:3000")
            .with_transport(transport)
            .build();
        assert!(result.is_ok());
    }
}
