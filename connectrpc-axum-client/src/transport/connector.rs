//! TLS connector setup for hyper HTTP client.
//!
//! This module provides utilities for creating HTTPS connectors with rustls.

use std::sync::Arc;

use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use rustls::ClientConfig;

/// Build an HTTPS connector with the given TLS configuration.
///
/// If no custom TLS config is provided, uses the default webpki roots.
pub fn build_https_connector(
    tls_config: Option<ClientConfig>,
) -> HttpsConnector<HttpConnector> {
    match tls_config {
        Some(config) => {
            HttpsConnectorBuilder::new()
                .with_tls_config(config)
                .https_or_http()
                .enable_all_versions()
                .build()
        }
        None => {
            HttpsConnectorBuilder::new()
                .with_webpki_roots()
                .https_or_http()
                .enable_all_versions()
                .build()
        }
    }
}

/// Build an HTTP-only connector (no TLS).
///
/// Use this for development/testing with `http://` URLs.
pub fn build_http_connector() -> HttpConnector {
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    connector
}

/// Create a default TLS client configuration with webpki roots.
pub fn default_tls_config() -> ClientConfig {
    ClientConfig::builder()
        .with_root_certificates(webpki_roots())
        .with_no_client_auth()
}

/// Get the webpki root certificate store.
fn webpki_roots() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    roots
}

/// Builder for custom TLS configurations.
///
/// This provides a convenient API for common TLS configuration patterns.
#[derive(Default)]
pub struct TlsConfigBuilder {
    /// Additional root certificates to trust.
    additional_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    /// Whether to disable certificate verification (dangerous!).
    danger_accept_invalid_certs: bool,
    /// Client certificate for mTLS.
    client_cert: Option<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )>,
}

impl TlsConfigBuilder {
    /// Create a new TLS config builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a root certificate to trust.
    ///
    /// The certificate should be in DER format.
    pub fn add_root_certificate(mut self, cert: rustls::pki_types::CertificateDer<'static>) -> Self {
        self.additional_roots.push(cert);
        self
    }

    /// Disable certificate verification.
    ///
    /// # Warning
    ///
    /// This is dangerous and should only be used for development/testing!
    /// It makes the connection vulnerable to man-in-the-middle attacks.
    pub fn danger_accept_invalid_certs(mut self) -> Self {
        self.danger_accept_invalid_certs = true;
        self
    }

    /// Set the client certificate for mTLS.
    ///
    /// The cert chain should be in DER format, with the client certificate first
    /// followed by any intermediate certificates.
    pub fn client_auth(
        mut self,
        cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        private_key: rustls::pki_types::PrivateKeyDer<'static>,
    ) -> Self {
        self.client_cert = Some((cert_chain, private_key));
        self
    }

    /// Build the TLS client configuration.
    pub fn build(self) -> Result<ClientConfig, rustls::Error> {
        let mut roots = webpki_roots();

        // Add any additional root certificates
        for cert in self.additional_roots {
            roots.add(cert)?;
        }

        let builder = ClientConfig::builder().with_root_certificates(roots);

        let config = if let Some((cert_chain, key)) = self.client_cert {
            builder.with_client_auth_cert(cert_chain, key)?
        } else {
            builder.with_no_client_auth()
        };

        Ok(config)
    }
}

/// A certificate verifier that accepts any certificate.
///
/// # Warning
///
/// This is extremely dangerous and should only be used for development/testing!
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
/// # Warning
///
/// This should only be used for development/testing!
pub fn danger_accept_invalid_certs_config() -> ClientConfig {
    ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DangerousAcceptAnyCertVerifier))
        .with_no_client_auth()
}
