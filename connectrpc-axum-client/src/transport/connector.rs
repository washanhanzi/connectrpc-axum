//! TLS connector setup for hyper HTTP client.
//!
//! This module provides utilities for creating HTTPS connectors with rustls.
//!
//! # Feature Flags
//!
//! TLS support requires both a crypto provider and root certificates:
//!
//! - **Crypto providers** (choose one):
//!   - `tls-ring` - Use ring crypto (default with `tls` feature)
//!   - `tls-aws-lc` - Use AWS LC crypto
//!
//! - **Root certificates** (choose one):
//!   - `tls-native-roots` - Use system root certificates (default with `tls` feature)
//!   - `tls-webpki-roots` - Use bundled Mozilla root certificates
//!
//! The `tls` feature enables `tls-ring` + `tls-native-roots` for convenience.
//!
//! # Example
//!
//! ```ignore
//! // With default `tls` feature, HTTPS just works:
//! let connector = build_https_connector(None);
//!
//! // For custom TLS config, use the type-state builder:
//! let config = TlsConfigBuilder::new()
//!     .with_ring()
//!     .with_native_roots()
//!     .build()?;
//! let connector = build_https_connector(Some(config));
//! ```

use std::marker::PhantomData;
use std::sync::Arc;

use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use rustls::ClientConfig;

// ============================================================================
// Auto TLS Configuration (feature-gated)
// ============================================================================

/// Check if TLS features are properly configured.
///
/// Returns true if both a crypto provider AND root certificates are available.
#[inline]
pub const fn has_tls_support() -> bool {
    cfg!(any(feature = "tls-ring", feature = "tls-aws-lc"))
        && cfg!(any(
            feature = "tls-native-roots",
            feature = "tls-webpki-roots"
        ))
}

/// Try to get a crypto provider ConfigBuilder.
///
/// Priority:
/// 1. Feature-gated provider (tls-ring or tls-aws-lc)
/// 2. User-installed global default provider
/// 3. None if no provider available
fn try_get_crypto_provider_builder() -> Option<rustls::ConfigBuilder<ClientConfig, rustls::WantsVerifier>> {
    // Priority 1: feature-gated providers
    #[cfg(feature = "tls-ring")]
    return Some({
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("safe default protocol versions should be valid")
    });

    #[cfg(all(feature = "tls-aws-lc", not(feature = "tls-ring")))]
    return Some({
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("safe default protocol versions should be valid")
    });

    // Priority 2: global default (when no feature-gated provider)
    #[cfg(not(any(feature = "tls-ring", feature = "tls-aws-lc")))]
    {
        rustls::crypto::CryptoProvider::get_default().map(|provider| {
            ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .expect("safe default protocol versions should be valid")
        })
    }
}

/// Build the default TLS configuration.
///
/// Uses feature-gated root certificates (native or webpki) and either
/// a feature-gated crypto provider or a user-installed global default.
///
/// Returns `None` if no crypto provider is available.
#[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
pub fn default_tls_config() -> Option<ClientConfig> {
    let builder = try_get_crypto_provider_builder()?;
    let roots = build_root_store();

    Some(
        builder
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// Build the root certificate store from enabled features.
#[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
fn build_root_store() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();

    // Load native roots if enabled (prefer native over webpki if both enabled)
    #[cfg(feature = "tls-native-roots")]
    {
        let native_certs = rustls_native_certs::load_native_certs();
        if !native_certs.errors.is_empty() {
            // Log errors but continue - some certs may have loaded successfully
            #[cfg(feature = "tracing")]
            tracing::debug!("errors loading native certs: {:?}", native_certs.errors);
        }
        roots.add_parsable_certificates(native_certs.certs);
    }

    // Load webpki roots if enabled and native roots are not
    #[cfg(all(feature = "tls-webpki-roots", not(feature = "tls-native-roots")))]
    {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    roots
}

// ============================================================================
// HTTPS Connector Builder
// ============================================================================

/// Build an HTTPS connector with the given TLS configuration.
///
/// If no custom TLS config is provided, attempts to build a default config using:
/// 1. Feature-gated crypto provider (tls-ring or tls-aws-lc), or
/// 2. User-installed global default provider
///
/// Combined with feature-gated root certificates (tls-native-roots or tls-webpki-roots).
///
/// # Panics
///
/// Panics if no TLS config can be built:
/// - No custom config provided, AND
/// - No root certificate features enabled, OR
/// - No crypto provider available (neither feature-gated nor global default)
pub fn build_https_connector(tls_config: Option<ClientConfig>) -> HttpsConnector<HttpConnector> {
    let config = match tls_config {
        Some(config) => config,
        None => {
            #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
            {
                default_tls_config().unwrap_or_else(|| {
                    panic!(
                        "HTTPS requires a crypto provider. Either:\n\
                         - Enable `tls-ring` or `tls-aws-lc` feature, or\n\
                         - Install a global crypto provider via `CryptoProvider::install_default()`\n\n\
                         Example in Cargo.toml:\n\
                         connectrpc-axum-client = {{ version = \"...\", features = [\"tls\"] }}"
                    );
                })
            }

            #[cfg(not(any(feature = "tls-native-roots", feature = "tls-webpki-roots")))]
            {
                panic!(
                    "HTTPS requires TLS root certificates. Enable one of:\n\
                     - `tls-native-roots` - use system certificates\n\
                     - `tls-webpki-roots` - use bundled Mozilla certificates\n\n\
                     Or enable `tls` feature for sensible defaults:\n\
                     connectrpc-axum-client = {{ version = \"...\", features = [\"tls\"] }}"
                );
            }
        }
    };

    HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_all_versions()
        .build()
}

/// Build an HTTP-only connector (no TLS).
///
/// Use this for development/testing with `http://` URLs.
pub fn build_http_connector() -> HttpConnector {
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    connector
}

// ============================================================================
// Type-State TLS Config Builder
// ============================================================================

/// Marker type: no crypto provider selected.
pub struct NoProvider;

/// Marker type: ring crypto provider selected.
#[cfg(feature = "tls-ring")]
pub struct RingProvider;

/// Marker type: AWS LC crypto provider selected.
#[cfg(feature = "tls-aws-lc")]
pub struct AwsLcProvider;

/// Marker type: no root certificates selected.
pub struct NoRoots;

/// Marker type: native root certificates selected.
#[cfg(feature = "tls-native-roots")]
pub struct NativeRoots;

/// Marker type: webpki root certificates selected.
#[cfg(feature = "tls-webpki-roots")]
pub struct WebpkiRoots;

/// Marker type: custom root certificates provided.
pub struct CustomRoots {
    store: rustls::RootCertStore,
}

/// Trait for types that provide a crypto provider.
///
/// Used by the type-state builder when user explicitly chooses a provider.
pub trait CryptoProvider {
    fn crypto_provider_builder() -> rustls::ConfigBuilder<ClientConfig, rustls::WantsVerifier>;
}

#[cfg(feature = "tls-ring")]
impl CryptoProvider for RingProvider {
    fn crypto_provider_builder() -> rustls::ConfigBuilder<ClientConfig, rustls::WantsVerifier> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("safe default protocol versions should be valid")
    }
}

#[cfg(feature = "tls-aws-lc")]
impl CryptoProvider for AwsLcProvider {
    fn crypto_provider_builder() -> rustls::ConfigBuilder<ClientConfig, rustls::WantsVerifier> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("safe default protocol versions should be valid")
    }
}

/// Trait for types that provide root certificates.
pub trait RootCertificates {
    fn root_store(&self) -> rustls::RootCertStore;
}

#[cfg(feature = "tls-native-roots")]
impl RootCertificates for NativeRoots {
    fn root_store(&self) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        let native_certs = rustls_native_certs::load_native_certs();
        roots.add_parsable_certificates(native_certs.certs);
        roots
    }
}

#[cfg(feature = "tls-webpki-roots")]
impl RootCertificates for WebpkiRoots {
    fn root_store(&self) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        roots
    }
}

impl RootCertificates for CustomRoots {
    fn root_store(&self) -> rustls::RootCertStore {
        self.store.clone()
    }
}

/// Type-state builder for custom TLS configurations.
///
/// This builder ensures at compile time that both a crypto provider
/// and root certificates are configured before building.
///
/// # Example
///
/// ```ignore
/// // Basic usage with feature-gated methods:
/// let config = TlsConfigBuilder::new()
///     .with_ring()           // requires tls-ring feature
///     .with_native_roots()   // requires tls-native-roots feature
///     .build()?;
///
/// // With custom root certificates:
/// let mut custom_roots = rustls::RootCertStore::empty();
/// custom_roots.add(my_cert)?;
///
/// let config = TlsConfigBuilder::new()
///     .with_ring()
///     .with_custom_roots(custom_roots)
///     .build()?;
///
/// // With client authentication (mTLS):
/// let config = TlsConfigBuilder::new()
///     .with_ring()
///     .with_native_roots()
///     .with_client_auth(cert_chain, private_key)
///     .build()?;
/// ```
pub struct TlsConfigBuilder<P, R> {
    roots: R,
    additional_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    client_auth: Option<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )>,
    _provider: PhantomData<P>,
}

impl TlsConfigBuilder<NoProvider, NoRoots> {
    /// Create a new TLS config builder.
    ///
    /// You must call a provider method (e.g., `with_ring()`) and a roots method
    /// (e.g., `with_native_roots()`) before calling `build()`.
    pub fn new() -> Self {
        Self {
            roots: NoRoots,
            additional_roots: Vec::new(),
            client_auth: None,
            _provider: PhantomData,
        }
    }
}

impl Default for TlsConfigBuilder<NoProvider, NoRoots> {
    fn default() -> Self {
        Self::new()
    }
}

// Provider selection methods (only available on NoProvider state)

#[cfg(feature = "tls-ring")]
impl<R> TlsConfigBuilder<NoProvider, R> {
    /// Use the ring crypto provider.
    ///
    /// Requires the `tls-ring` feature.
    pub fn with_ring(self) -> TlsConfigBuilder<RingProvider, R> {
        TlsConfigBuilder {
            roots: self.roots,
            additional_roots: self.additional_roots,
            client_auth: self.client_auth,
            _provider: PhantomData,
        }
    }
}

#[cfg(feature = "tls-aws-lc")]
impl<R> TlsConfigBuilder<NoProvider, R> {
    /// Use the AWS LC crypto provider.
    ///
    /// Requires the `tls-aws-lc` feature.
    pub fn with_aws_lc(self) -> TlsConfigBuilder<AwsLcProvider, R> {
        TlsConfigBuilder {
            roots: self.roots,
            additional_roots: self.additional_roots,
            client_auth: self.client_auth,
            _provider: PhantomData,
        }
    }
}

// Root certificate selection methods (only available on NoRoots state)

#[cfg(feature = "tls-native-roots")]
impl<P> TlsConfigBuilder<P, NoRoots> {
    /// Use the system's native root certificates.
    ///
    /// Requires the `tls-native-roots` feature.
    pub fn with_native_roots(self) -> TlsConfigBuilder<P, NativeRoots> {
        TlsConfigBuilder {
            roots: NativeRoots,
            additional_roots: self.additional_roots,
            client_auth: self.client_auth,
            _provider: PhantomData,
        }
    }
}

#[cfg(feature = "tls-webpki-roots")]
impl<P> TlsConfigBuilder<P, NoRoots> {
    /// Use the bundled Mozilla root certificates.
    ///
    /// Requires the `tls-webpki-roots` feature.
    pub fn with_webpki_roots(self) -> TlsConfigBuilder<P, WebpkiRoots> {
        TlsConfigBuilder {
            roots: WebpkiRoots,
            additional_roots: self.additional_roots,
            client_auth: self.client_auth,
            _provider: PhantomData,
        }
    }
}

impl<P> TlsConfigBuilder<P, NoRoots> {
    /// Use custom root certificates.
    ///
    /// This is always available regardless of feature flags.
    pub fn with_custom_roots(
        self,
        store: rustls::RootCertStore,
    ) -> TlsConfigBuilder<P, CustomRoots> {
        TlsConfigBuilder {
            roots: CustomRoots { store },
            additional_roots: self.additional_roots,
            client_auth: self.client_auth,
            _provider: PhantomData,
        }
    }
}

// Additional configuration methods (available on any state)

impl<P, R> TlsConfigBuilder<P, R> {
    /// Add an additional root certificate to trust.
    ///
    /// This is useful for adding private CA certificates in addition to
    /// the standard roots.
    pub fn add_root_certificate(
        mut self,
        cert: rustls::pki_types::CertificateDer<'static>,
    ) -> Self {
        self.additional_roots.push(cert);
        self
    }

    /// Set client authentication credentials for mTLS.
    ///
    /// The cert chain should contain the client certificate first,
    /// followed by any intermediate certificates.
    pub fn with_client_auth(
        mut self,
        cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        private_key: rustls::pki_types::PrivateKeyDer<'static>,
    ) -> Self {
        self.client_auth = Some((cert_chain, private_key));
        self
    }
}

// Build method (only available when both provider and roots are set)

impl<P: CryptoProvider, R: RootCertificates> TlsConfigBuilder<P, R> {
    /// Build the TLS client configuration.
    ///
    /// This method is only available when both a crypto provider and
    /// root certificates have been configured.
    pub fn build(self) -> Result<ClientConfig, rustls::Error> {
        let mut roots = self.roots.root_store();

        // Add any additional root certificates
        for cert in self.additional_roots {
            roots.add(cert)?;
        }

        let builder = P::crypto_provider_builder().with_root_certificates(roots);

        let config = match self.client_auth {
            Some((cert_chain, key)) => builder.with_client_auth_cert(cert_chain, key)?,
            None => builder.with_no_client_auth(),
        };

        Ok(config)
    }
}

// ============================================================================
// Dangerous: Accept Invalid Certificates
// ============================================================================

/// A certificate verifier that accepts any certificate.
///
/// # Warning
///
/// This is extremely dangerous and should only be used for development/testing!
/// It makes the connection vulnerable to man-in-the-middle attacks.
#[derive(Debug)]
pub struct DangerousAcceptAnyCertVerifier;

impl rustls::client::danger::ServerCertVerifier for DangerousAcceptAnyCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

/// Create a TLS config that accepts any certificate (dangerous!).
///
/// This requires a crypto provider to be available, either via feature flags
/// or a user-installed global default.
///
/// # Warning
///
/// This should only be used for development/testing!
///
/// # Panics
///
/// Panics if no crypto provider is available.
pub fn danger_accept_invalid_certs_config() -> ClientConfig {
    let builder = try_get_crypto_provider_builder().unwrap_or_else(|| {
        panic!(
            "danger_accept_invalid_certs_config requires a crypto provider. Either:\n\
             - Enable `tls-ring` or `tls-aws-lc` feature, or\n\
             - Install a global crypto provider via `CryptoProvider::install_default()`"
        );
    });

    builder
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DangerousAcceptAnyCertVerifier))
        .with_no_client_auth()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_tls_support() {
        // This will be true or false depending on enabled features
        let _ = has_tls_support();
    }

    #[cfg(all(
        any(feature = "tls-ring", feature = "tls-aws-lc"),
        any(feature = "tls-native-roots", feature = "tls-webpki-roots")
    ))]
    #[test]
    fn test_default_tls_config() {
        let config = default_tls_config().expect("should build with features enabled");
        // Basic sanity check - config was created successfully
        assert!(config.alpn_protocols.is_empty()); // We don't set ALPN by default
    }

    #[cfg(all(
        any(feature = "tls-ring", feature = "tls-aws-lc"),
        any(feature = "tls-native-roots", feature = "tls-webpki-roots")
    ))]
    #[test]
    fn test_build_https_connector_default() {
        // Should not panic when TLS features are enabled
        let _ = build_https_connector(None);
    }

    #[test]
    fn test_build_http_connector() {
        let connector = build_http_connector();
        // Basic sanity check
        let _ = connector;
    }

    #[cfg(all(feature = "tls-ring", feature = "tls-native-roots"))]
    #[test]
    fn test_type_state_builder_ring_native() {
        let config = TlsConfigBuilder::new()
            .with_ring()
            .with_native_roots()
            .build()
            .expect("should build successfully");
        let _ = config;
    }

    #[cfg(all(feature = "tls-ring", feature = "tls-webpki-roots"))]
    #[test]
    fn test_type_state_builder_ring_webpki() {
        let config = TlsConfigBuilder::new()
            .with_ring()
            .with_webpki_roots()
            .build()
            .expect("should build successfully");
        let _ = config;
    }

    #[cfg(feature = "tls-ring")]
    #[test]
    fn test_type_state_builder_custom_roots() {
        let custom_roots = rustls::RootCertStore::empty();
        let config = TlsConfigBuilder::new()
            .with_ring()
            .with_custom_roots(custom_roots)
            .build()
            .expect("should build successfully");
        let _ = config;
    }
}
