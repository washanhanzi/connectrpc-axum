//! Hyper-based HTTP transport for Connect RPC client.
//!
//! This module provides [`HyperTransport`], the main HTTP transport implementation
//! using hyper_util's legacy client.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use hyper::body::Incoming;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::{TokioExecutor, TokioTimer};
use rustls::ClientConfig;
use tower_service::Service;

use super::body::TransportBody;
use super::connector::{build_https_connector, default_tls_config, danger_accept_invalid_certs_config};
use crate::ClientError;

/// Type alias for the hyper client with HTTPS connector.
type HyperClient = Client<HttpsConnector<HttpConnector>, TransportBody>;

/// HTTP transport using hyper_util's legacy client.
///
/// This transport provides full HTTP/1.1 and HTTP/2 support with TLS,
/// connection pooling, and automatic protocol negotiation via ALPN.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::transport::HyperTransport;
///
/// let transport = HyperTransport::builder()
///     .build()?;
///
/// // Use with ConnectClient
/// let client = ConnectClient::builder("https://api.example.com")
///     .with_transport(transport)
///     .build()?;
/// ```
#[derive(Clone)]
pub struct HyperTransport {
    client: HyperClient,
    /// Whether HTTP/2 only mode is enabled.
    http2_only: bool,
}

impl std::fmt::Debug for HyperTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HyperTransport")
            .field("http2_only", &self.http2_only)
            .finish_non_exhaustive()
    }
}

impl HyperTransport {
    /// Create a new transport builder.
    pub fn builder() -> HyperTransportBuilder {
        HyperTransportBuilder::new()
    }

    /// Create a new transport with default settings.
    pub fn new() -> Result<Self, ClientError> {
        Self::builder().build()
    }

    /// Send an HTTP request and receive a response.
    pub async fn request(
        &self,
        request: http::Request<TransportBody>,
    ) -> Result<http::Response<Incoming>, ClientError> {
        self.client
            .request(request)
            .await
            .map_err(|e| ClientError::Transport(format!("request failed: {}", e)))
    }

    /// Check if this transport is configured for HTTP/2 only.
    pub fn is_http2_only(&self) -> bool {
        self.http2_only
    }
}

impl Default for HyperTransport {
    fn default() -> Self {
        Self::new().expect("failed to create default HyperTransport")
    }
}

/// Builder for [`HyperTransport`].
///
/// Provides configuration options for the HTTP transport including
/// TLS settings, HTTP/2 options, and connection pooling.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::transport::HyperTransportBuilder;
/// use std::time::Duration;
///
/// let transport = HyperTransportBuilder::new()
///     .http2_only(true)
///     .pool_idle_timeout(Duration::from_secs(90))
///     .build()?;
/// ```
pub struct HyperTransportBuilder {
    /// Custom TLS configuration.
    tls_config: Option<ClientConfig>,
    /// Force HTTP/2 only (for h2c or when HTTP/2 is required).
    http2_only: bool,
    /// Connection pool idle timeout.
    pool_idle_timeout: Option<Duration>,
    /// Maximum idle connections per host.
    pool_max_idle_per_host: usize,
    /// HTTP/2 initial stream window size.
    h2_initial_stream_window_size: Option<u32>,
    /// HTTP/2 initial connection window size.
    h2_initial_connection_window_size: Option<u32>,
    /// HTTP/2 keep-alive interval.
    h2_keep_alive_interval: Option<Duration>,
    /// HTTP/2 keep-alive timeout.
    h2_keep_alive_timeout: Option<Duration>,
    /// Whether to accept invalid certificates (dangerous!).
    danger_accept_invalid_certs: bool,
}

impl Default for HyperTransportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HyperTransportBuilder {
    /// Create a new transport builder with default settings.
    pub fn new() -> Self {
        Self {
            tls_config: None,
            http2_only: false,
            pool_idle_timeout: Some(Duration::from_secs(90)),
            pool_max_idle_per_host: 32,
            h2_initial_stream_window_size: None,
            h2_initial_connection_window_size: None,
            h2_keep_alive_interval: None,
            h2_keep_alive_timeout: None,
            danger_accept_invalid_certs: false,
        }
    }

    /// Set a custom TLS configuration.
    ///
    /// Use this to configure custom root certificates, client certificates for mTLS,
    /// or other TLS settings.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use rustls::ClientConfig;
    ///
    /// let tls_config = ClientConfig::builder()
    ///     .with_root_certificates(my_roots)
    ///     .with_no_client_auth();
    ///
    /// let transport = HyperTransportBuilder::new()
    ///     .tls_config(tls_config)
    ///     .build()?;
    /// ```
    pub fn tls_config(mut self, config: ClientConfig) -> Self {
        self.tls_config = Some(config);
        self
    }

    /// Enable HTTP/2 only mode.
    ///
    /// When enabled, the transport will use HTTP/2 directly without
    /// the HTTP/1.1 upgrade handshake. This is required for:
    /// - Bidirectional streaming over unencrypted connections (h2c)
    /// - When you know the server only supports HTTP/2
    ///
    /// For HTTPS connections, HTTP/2 is typically negotiated via ALPN,
    /// so this setting is often not needed.
    pub fn http2_only(mut self, enabled: bool) -> Self {
        self.http2_only = enabled;
        self
    }

    /// Set the connection pool idle timeout.
    ///
    /// Connections that have been idle for longer than this duration
    /// will be closed and removed from the pool.
    ///
    /// Default: 90 seconds.
    pub fn pool_idle_timeout(mut self, timeout: Duration) -> Self {
        self.pool_idle_timeout = Some(timeout);
        self
    }

    /// Disable connection pool idle timeout.
    ///
    /// Connections will not be closed due to inactivity.
    pub fn pool_idle_timeout_none(mut self) -> Self {
        self.pool_idle_timeout = None;
        self
    }

    /// Set the maximum number of idle connections per host.
    ///
    /// Default: 32.
    pub fn pool_max_idle_per_host(mut self, max: usize) -> Self {
        self.pool_max_idle_per_host = max;
        self
    }

    /// Set the HTTP/2 initial stream window size.
    ///
    /// This controls flow control at the stream level.
    /// Larger values may improve throughput for high-latency connections.
    pub fn h2_initial_stream_window_size(mut self, size: u32) -> Self {
        self.h2_initial_stream_window_size = Some(size);
        self
    }

    /// Set the HTTP/2 initial connection window size.
    ///
    /// This controls flow control at the connection level.
    /// Larger values may improve throughput for multiplexed streams.
    pub fn h2_initial_connection_window_size(mut self, size: u32) -> Self {
        self.h2_initial_connection_window_size = Some(size);
        self
    }

    /// Set the HTTP/2 keep-alive interval.
    ///
    /// If set, the transport will send HTTP/2 PING frames at this interval
    /// to keep the connection alive and detect dead connections.
    pub fn h2_keep_alive_interval(mut self, interval: Duration) -> Self {
        self.h2_keep_alive_interval = Some(interval);
        self
    }

    /// Set the HTTP/2 keep-alive timeout.
    ///
    /// How long to wait for a PING response before considering the connection dead.
    /// Only effective if `h2_keep_alive_interval` is also set.
    pub fn h2_keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.h2_keep_alive_timeout = Some(timeout);
        self
    }

    /// Accept invalid TLS certificates.
    ///
    /// # Warning
    ///
    /// This is extremely dangerous and should only be used for development/testing!
    /// It makes the connection vulnerable to man-in-the-middle attacks.
    pub fn danger_accept_invalid_certs(mut self) -> Self {
        self.danger_accept_invalid_certs = true;
        self
    }

    /// Build the transport.
    pub fn build(self) -> Result<HyperTransport, ClientError> {
        // Create TLS config
        let tls_config = if self.danger_accept_invalid_certs {
            danger_accept_invalid_certs_config()
        } else {
            self.tls_config.unwrap_or_else(default_tls_config)
        };

        // Create HTTPS connector
        let https_connector = build_https_connector(Some(tls_config));

        // Create client builder
        let mut builder = Client::builder(TokioExecutor::new());

        // Configure connection pool timer (required for pool_idle_timeout to work)
        builder.pool_timer(TokioTimer::new());

        // Configure connection pool
        if let Some(timeout) = self.pool_idle_timeout {
            builder.pool_idle_timeout(timeout);
        }
        builder.pool_max_idle_per_host(self.pool_max_idle_per_host);

        // Configure HTTP/2
        if self.http2_only {
            builder.http2_only(true);
        }

        if let Some(size) = self.h2_initial_stream_window_size {
            builder.http2_initial_stream_window_size(size);
        }

        if let Some(size) = self.h2_initial_connection_window_size {
            builder.http2_initial_connection_window_size(size);
        }

        if let Some(interval) = self.h2_keep_alive_interval {
            builder.http2_keep_alive_interval(interval);
        }

        if let Some(timeout) = self.h2_keep_alive_timeout {
            builder.http2_keep_alive_timeout(timeout);
        }

        // Build client
        let client = builder.build(https_connector);

        Ok(HyperTransport {
            client,
            http2_only: self.http2_only,
        })
    }
}

impl std::fmt::Debug for HyperTransportBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HyperTransportBuilder")
            .field("tls_config", &self.tls_config.is_some())
            .field("http2_only", &self.http2_only)
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("pool_max_idle_per_host", &self.pool_max_idle_per_host)
            .field("h2_initial_stream_window_size", &self.h2_initial_stream_window_size)
            .field("h2_initial_connection_window_size", &self.h2_initial_connection_window_size)
            .field("h2_keep_alive_interval", &self.h2_keep_alive_interval)
            .field("h2_keep_alive_timeout", &self.h2_keep_alive_timeout)
            .field("danger_accept_invalid_certs", &self.danger_accept_invalid_certs)
            .finish()
    }
}

// Implement tower::Service for HyperTransport
impl Service<http::Request<TransportBody>> for HyperTransport {
    type Response = http::Response<Incoming>;
    type Error = ClientError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // hyper_util legacy::Client is always ready
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<TransportBody>) -> Self::Future {
        let client = self.client.clone();
        Box::pin(async move {
            client
                .request(req)
                .await
                .map_err(|e| ClientError::Transport(format!("request failed: {}", e)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = HyperTransportBuilder::new();
        assert!(!builder.http2_only);
        assert_eq!(builder.pool_max_idle_per_host, 32);
        assert!(builder.pool_idle_timeout.is_some());
    }

    #[test]
    fn test_builder_http2_only() {
        let builder = HyperTransportBuilder::new().http2_only(true);
        assert!(builder.http2_only);
    }

    #[test]
    fn test_builder_pool_settings() {
        let builder = HyperTransportBuilder::new()
            .pool_idle_timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(10);
        assert_eq!(builder.pool_idle_timeout, Some(Duration::from_secs(60)));
        assert_eq!(builder.pool_max_idle_per_host, 10);
    }

    #[test]
    fn test_builder_h2_settings() {
        let builder = HyperTransportBuilder::new()
            .h2_initial_stream_window_size(1024 * 1024)
            .h2_initial_connection_window_size(2 * 1024 * 1024)
            .h2_keep_alive_interval(Duration::from_secs(10))
            .h2_keep_alive_timeout(Duration::from_secs(5));

        assert_eq!(builder.h2_initial_stream_window_size, Some(1024 * 1024));
        assert_eq!(builder.h2_initial_connection_window_size, Some(2 * 1024 * 1024));
        assert_eq!(builder.h2_keep_alive_interval, Some(Duration::from_secs(10)));
        assert_eq!(builder.h2_keep_alive_timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_build_transport() {
        let result = HyperTransportBuilder::new().build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_transport_http2_only() {
        let result = HyperTransportBuilder::new()
            .http2_only(true)
            .build();
        assert!(result.is_ok());
        assert!(result.unwrap().is_http2_only());
    }
}
