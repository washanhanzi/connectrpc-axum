//! HTTP transport layer for Connect RPC client.
//!
//! This module provides the [`HyperTransport`] type, which handles HTTP communication
//! using hyper_util's legacy client. It supports:
//!
//! - HTTP/1.1 and HTTP/2 with automatic protocol negotiation
//! - TLS with rustls
//! - Connection pooling
//! - Tower service integration for middleware
//!
//! # Example
//!
//! ```ignore
//! use connectrpc_axum_client::transport::{HyperTransport, HyperTransportBuilder};
//! use std::time::Duration;
//!
//! // Create with default settings
//! let transport = HyperTransport::new()?;
//!
//! // Or use the builder for customization
//! let transport = HyperTransportBuilder::new()
//!     .http2_only(true)
//!     .pool_idle_timeout(Duration::from_secs(60))
//!     .build()?;
//! ```

mod body;
mod connector;
mod hyper;

pub use body::TransportBody;
pub use connector::{
    build_https_connector,
    build_http_connector,
    default_tls_config,
    danger_accept_invalid_certs_config,
    TlsConfigBuilder,
    DangerousAcceptAnyCertVerifier,
};
pub use hyper::{HyperTransport, HyperTransportBuilder};

// Re-export rustls types that users might need for TLS configuration
pub use rustls::ClientConfig as TlsClientConfig;
