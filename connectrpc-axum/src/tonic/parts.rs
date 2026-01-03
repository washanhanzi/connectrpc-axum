//! Request parts capture for FromRequestParts extraction in tonic handlers.
//!
//! This module provides types for capturing HTTP request parts before tonic consumes them,
//! enabling axum's `FromRequestParts` extractors to work with tonic gRPC handlers.

use std::task::{Context, Poll};

use tower::{Layer, Service};

/// Full HTTP request context for extractor support.
///
/// Owns all parts needed for `FromRequestParts` extraction. The key insight is that
/// `extensions` is moved (not cloned), enabling `Extension<T>` to work without requiring
/// `T: Clone` beyond axum's existing requirements.
pub struct RequestContext {
    pub method: http::Method,
    pub uri: http::Uri,
    pub version: http::Version,
    pub headers: http::HeaderMap,
    pub extensions: http::Extensions,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            method: http::Method::POST,
            uri: http::Uri::default(),
            version: http::Version::HTTP_2,
            headers: http::HeaderMap::new(),
            extensions: http::Extensions::new(),
        }
    }
}

impl RequestContext {
    /// Convert into `http::request::Parts` for `FromRequestParts` extraction.
    ///
    /// This consumes the `RequestContext` and produces parts suitable for
    /// axum extractors.
    pub fn into_parts(self) -> http::request::Parts {
        // Create a dummy request to get properly initialized Parts
        let (mut parts, _body) = http::Request::new(()).into_parts();
        parts.method = self.method;
        parts.uri = self.uri;
        parts.version = self.version;
        parts.headers = self.headers;
        parts.extensions = self.extensions;
        parts
    }
}

/// Cloneable subset of request parts captured by `FromRequestPartsLayer`.
///
/// This struct captures the parts of an HTTP request that can be cloned and stored
/// in request extensions. It's used by the middleware to preserve request metadata
/// before tonic consumes the request.
///
/// Note: `extensions` is NOT included here because `http::Extensions` doesn't implement
/// `Clone`. The extensions are accessed via `tonic::Request::into_parts()` which gives
/// ownership of the extensions.
#[derive(Clone)]
pub struct CapturedParts {
    pub method: http::Method,
    pub uri: http::Uri,
    pub version: http::Version,
    pub headers: http::HeaderMap,
}

impl Default for CapturedParts {
    fn default() -> Self {
        Self {
            method: http::Method::POST,
            uri: http::Uri::default(),
            version: http::Version::HTTP_2,
            headers: http::HeaderMap::new(),
        }
    }
}

/// Tower layer that enables `FromRequestParts` extractors in tonic handlers.
///
/// This middleware clones the cloneable parts of the HTTP request (method, uri, version,
/// headers) and stores them in the request extensions. The tonic service can then
/// retrieve these parts and combine them with the owned extensions from the tonic request
/// to build a complete `RequestContext` for extractor support.
///
/// When `enabled` is false, the layer still wraps the service but skips the capture work,
/// allowing conditional application without changing the service type.
#[derive(Clone, Copy, Debug)]
pub struct FromRequestPartsLayer {
    enabled: bool,
}

impl Default for FromRequestPartsLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl FromRequestPartsLayer {
    /// Create a new layer that enables `FromRequestParts` extractors.
    pub fn new() -> Self {
        Self { enabled: true }
    }

    /// Create a layer with explicit enabled/disabled state.
    ///
    /// When disabled, the layer wraps the service but skips the capture work,
    /// avoiding the overhead of cloning headers when not needed.
    pub fn enabled(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl<S> Layer<S> for FromRequestPartsLayer {
    type Service = FromRequestPartsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FromRequestPartsService {
            inner,
            enabled: self.enabled,
        }
    }
}

/// Service that captures request parts before forwarding to inner service.
///
/// When `enabled` is true, clones method, uri, version, and headers into extensions.
/// When `enabled` is false, passes through without any overhead beyond a bool check.
#[derive(Clone, Debug)]
pub struct FromRequestPartsService<S> {
    inner: S,
    enabled: bool,
}

impl<S, B> Service<http::Request<B>> for FromRequestPartsService<S>
where
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: http::Request<B>) -> Self::Future {
        if self.enabled {
            // Capture cloneable parts
            let captured = CapturedParts {
                method: request.method().clone(),
                uri: request.uri().clone(),
                version: request.version(),
                headers: request.headers().clone(),
            };

            // Store in extensions - will survive into tonic::Request
            request.extensions_mut().insert(captured);
        }

        self.inner.call(request)
    }
}
