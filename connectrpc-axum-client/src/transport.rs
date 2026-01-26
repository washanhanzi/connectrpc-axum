//! HTTP transport layer for Connect RPC client.
//!
//! This module provides the [`HyperTransport`] type, which handles HTTP communication
//! using hyper_util's legacy client. It supports:
//!
//! - HTTP/1.1 and HTTP/2 with automatic protocol negotiation
//! - TLS with rustls (feature-gated)
//! - Connection pooling
//! - Tower service integration for middleware
//!
//! # Feature Flags
//!
//! TLS support requires enabling the appropriate features:
//!
//! - `tls` (default) - Enables `tls-ring` + `tls-native-roots` for convenience
//! - `tls-ring` / `tls-aws-lc` - Crypto providers
//! - `tls-native-roots` / `tls-webpki-roots` - Root certificates
//!
//! # Example
//!
//! ```ignore
//! use connectrpc_axum_client::transport::{HyperTransport, HyperTransportBuilder};
//! use std::time::Duration;
//!
//! // Create with default settings (uses default TLS if features enabled)
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
    build_http_connector,
    build_https_connector,
    danger_accept_invalid_certs_config,
    has_tls_support,
    DangerousAcceptAnyCertVerifier,
    TlsConfigBuilder,
    // Type-state marker types
    NoProvider,
    NoRoots,
    CustomRoots,
    // Traits
    CryptoProvider,
    RootCertificates,
};

// Feature-gated exports
#[cfg(feature = "tls-ring")]
pub use connector::RingProvider;

#[cfg(feature = "tls-aws-lc")]
pub use connector::AwsLcProvider;

#[cfg(feature = "tls-native-roots")]
pub use connector::NativeRoots;

#[cfg(feature = "tls-webpki-roots")]
pub use connector::WebpkiRoots;

#[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
pub use connector::default_tls_config;

pub use hyper::{HyperTransport, HyperTransportBuilder};

// Re-export rustls types that users might need for TLS configuration
pub use rustls::ClientConfig as TlsClientConfig;
