//! Client builder for Connect RPC client.
//!
//! Provides a fluent API for configuring and building a [`ConnectClient`].

use crate::client::ConnectClient;
use connectrpc_axum_core::{CompressionConfig, CompressionEncoding};
use reqwest::Client;
use reqwest_middleware::{ClientBuilder as MiddlewareClientBuilder, ClientWithMiddleware, Middleware};
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
    /// Optional pre-configured reqwest client.
    client: Option<Client>,
    /// Middleware to add to the client.
    middleware: Vec<Arc<dyn Middleware>>,
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
    /// Enable HTTP/2 prior knowledge (h2c) for unencrypted HTTP/2 connections.
    http2_prior_knowledge: bool,
    /// TCP keep-alive interval for connections.
    tcp_keepalive: Option<Duration>,
}

impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("base_url", &self.base_url)
            .field("client", &self.client.is_some())
            .field("middleware_count", &self.middleware.len())
            .field("use_proto", &self.use_proto)
            .field("compression", &self.compression)
            .field("request_encoding", &self.request_encoding)
            .field("accept_encoding", &self.accept_encoding)
            .field("default_timeout", &self.default_timeout)
            .field("http2_prior_knowledge", &self.http2_prior_knowledge)
            .field("tcp_keepalive", &self.tcp_keepalive)
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
            client: None,
            middleware: Vec::new(),
            use_proto: false, // Default to JSON for broader compatibility
            compression: None,
            request_encoding: CompressionEncoding::Identity,
            accept_encoding: None,
            default_timeout: None,
            http2_prior_knowledge: false,
            tcp_keepalive: None,
        }
    }

    /// Use a pre-configured reqwest Client.
    ///
    /// This allows you to configure TLS, timeouts, connection pooling, etc.
    /// on the underlying HTTP client.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let http_client = reqwest::Client::builder()
    ///     .timeout(Duration::from_secs(30))
    ///     .build()?;
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .client(http_client)
    ///     .build()?;
    /// ```
    pub fn client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Add middleware to the client.
    ///
    /// Middleware is applied in the order it's added.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use reqwest_middleware::Middleware;
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .with_middleware(MyRetryMiddleware::new())
    ///     .with_middleware(MyLoggingMiddleware::new())
    ///     .build()?;
    /// ```
    pub fn with_middleware<M: Middleware>(mut self, middleware: M) -> Self {
        self.middleware.push(Arc::new(middleware));
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
    /// This timeout is propagated to the server via the `Connect-Timeout-Ms` header,
    /// allowing the server to cancel processing if the deadline will be exceeded.
    ///
    /// The timeout applies to the entire RPC call, including connection time,
    /// request sending, server processing, and response receiving.
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
    /// **Note:** This setting only applies when the builder creates the HTTP client.
    /// If you provide your own client via [`client()`], configure HTTP/2 on that
    /// client's builder instead.
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
    /// [`client()`]: Self::client
    pub fn http2_prior_knowledge(mut self) -> Self {
        self.http2_prior_knowledge = true;
        self
    }

    /// Set TCP keep-alive interval for connections.
    ///
    /// TCP keep-alive probes help detect dead connections and keep connections
    /// alive through NAT/firewall timeouts. This is especially useful for:
    /// - Long-running streaming RPCs
    /// - Connections that may be idle between requests
    /// - Networks with aggressive NAT timeout policies
    ///
    /// The duration specifies how long a connection can be idle before TCP
    /// starts sending keep-alive probes.
    ///
    /// **Note:** This setting only applies when the builder creates the HTTP client.
    /// If you provide your own client via [`client()`], configure TCP keep-alive
    /// on that client's builder instead.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new("http://localhost:3000")
    ///     .tcp_keepalive(Duration::from_secs(60))
    ///     .build()?;
    /// ```
    ///
    /// [`client()`]: Self::client
    pub fn tcp_keepalive(mut self, interval: Duration) -> Self {
        self.tcp_keepalive = Some(interval);
        self
    }

    /// Build the ConnectClient.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn build(self) -> Result<ConnectClient, ClientBuildError> {
        // Create or use provided reqwest client
        let base_client = match self.client {
            Some(c) => c,
            None => {
                let mut builder = Client::builder();
                if self.http2_prior_knowledge {
                    builder = builder.http2_prior_knowledge();
                }
                if let Some(keepalive) = self.tcp_keepalive {
                    builder = builder.tcp_keepalive(keepalive);
                }
                builder
                    .build()
                    .map_err(|e| ClientBuildError::HttpClient(e.to_string()))?
            }
        };

        // Apply middleware
        let http: ClientWithMiddleware = if self.middleware.is_empty() {
            MiddlewareClientBuilder::new(base_client).build()
        } else {
            let mut builder = MiddlewareClientBuilder::new(base_client);
            for mw in self.middleware {
                builder = builder.with_arc(mw);
            }
            builder.build()
        };

        // Normalize base URL (remove trailing slash)
        let base_url = self.base_url.trim_end_matches('/').to_string();

        Ok(ConnectClient::new(
            http,
            base_url,
            self.use_proto,
            self.compression.unwrap_or_default(),
            self.request_encoding,
            self.accept_encoding,
            self.default_timeout,
        ))
    }
}

/// Error type for client building failures.
#[derive(Debug, thiserror::Error)]
pub enum ClientBuildError {
    /// Failed to create HTTP client.
    #[error("failed to create HTTP client: {0}")]
    HttpClient(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = ClientBuilder::new("http://localhost:3000");
        assert!(!builder.use_proto);
        assert!(builder.client.is_none());
        assert!(builder.middleware.is_empty());
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
        let builder = ClientBuilder::new("http://localhost:3000")
            .accept_encoding(CompressionEncoding::Gzip);
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
        let client = ClientBuilder::new("http://localhost:3000/").build().unwrap();
        // The trailing slash should be removed
        assert!(!client.base_url().ends_with('/'));
    }

    #[test]
    fn test_builder_timeout() {
        let builder = ClientBuilder::new("http://localhost:3000")
            .timeout(Duration::from_secs(30));
        assert_eq!(builder.default_timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_builder_timeout_default_none() {
        let builder = ClientBuilder::new("http://localhost:3000");
        assert!(builder.default_timeout.is_none());
    }

    #[test]
    fn test_builder_http2_prior_knowledge_default_false() {
        let builder = ClientBuilder::new("http://localhost:3000");
        assert!(!builder.http2_prior_knowledge);
    }

    #[test]
    fn test_builder_http2_prior_knowledge() {
        let builder = ClientBuilder::new("http://localhost:3000").http2_prior_knowledge();
        assert!(builder.http2_prior_knowledge);
    }

    #[test]
    fn test_builder_http2_prior_knowledge_build() {
        // Verify that build() succeeds with http2_prior_knowledge enabled
        let result = ClientBuilder::new("http://localhost:3000")
            .http2_prior_knowledge()
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_tcp_keepalive_default_none() {
        let builder = ClientBuilder::new("http://localhost:3000");
        assert!(builder.tcp_keepalive.is_none());
    }

    #[test]
    fn test_builder_tcp_keepalive() {
        let builder =
            ClientBuilder::new("http://localhost:3000").tcp_keepalive(Duration::from_secs(60));
        assert_eq!(builder.tcp_keepalive, Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_builder_tcp_keepalive_build() {
        // Verify that build() succeeds with tcp_keepalive set
        let result = ClientBuilder::new("http://localhost:3000")
            .tcp_keepalive(Duration::from_secs(30))
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_combined_transport_options() {
        // Verify that multiple transport options can be combined
        let result = ClientBuilder::new("http://localhost:3000")
            .http2_prior_knowledge()
            .tcp_keepalive(Duration::from_secs(60))
            .timeout(Duration::from_secs(30))
            .build();
        assert!(result.is_ok());
    }
}
