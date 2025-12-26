//! Middleware layer for Connect RPC protocol handling.
//!
//! The [`ConnectLayer`] middleware detects the protocol variant from incoming requests
//! and stores it in request extensions so that response encoding can match the request format.

use crate::limits::MessageLimits;
use crate::protocol::RequestProtocol;
use axum::http::{header, Method, Request};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service, ServiceExt};

/// Layer that wraps services with Connect protocol detection and message limits.
///
/// This layer:
/// 1. Detects the protocol variant from the request (Content-Type header or query params)
/// 2. Stores the [`RequestProtocol`] and [`MessageLimits`] in request extensions
/// 3. Handler wrappers extract the protocol and inject it into response types
/// 4. Request extractors enforce message size limits
///
/// # Example
///
/// ```rust,ignore
/// use connectrpc_axum::{ConnectLayer, MessageLimits};
///
/// // Use default 4 MB limit
/// let router = Router::new()
///     .route("/service/Method", post(handler))
///     .layer(ConnectLayer::new());
///
/// // Custom 16 MB limit
/// let router = Router::new()
///     .route("/service/Method", post(handler))
///     .layer(ConnectLayer::with_limits(MessageLimits::new(16 * 1024 * 1024)));
/// ```
///
#[derive(Debug, Clone, Copy)]
pub struct ConnectLayer {
    limits: MessageLimits,
}

impl Default for ConnectLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectLayer {
    /// Create a new ConnectLayer with default message limits (4 MB).
    pub fn new() -> Self {
        Self {
            limits: MessageLimits::default(),
        }
    }

    /// Create a new ConnectLayer with custom message limits.
    pub fn with_limits(limits: MessageLimits) -> Self {
        Self { limits }
    }
}

impl<S> Layer<S> for ConnectLayer {
    type Service = ConnectService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectService {
            inner,
            limits: self.limits,
        }
    }
}

/// Service wrapper that provides per-request protocol context and message limits.
#[derive(Debug, Clone)]
pub struct ConnectService<S> {
    inner: S,
    limits: MessageLimits,
}

impl<S, ReqBody> Service<Request<ReqBody>> for ConnectService<S>
where
    S: Service<Request<ReqBody>> + Clone + Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Detect protocol from request and store in extensions
        let protocol = detect_protocol(&req);
        req.extensions_mut().insert(protocol);

        // Store message limits in extensions for request extractors
        req.extensions_mut().insert(self.limits);

        // Clone inner service for the async block
        let inner = self.inner.clone();
        // Replace self.inner with the clone so it's ready for the next request
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move { inner.oneshot(req).await })
    }
}

/// Detect the protocol variant from an incoming request.
fn detect_protocol<B>(req: &Request<B>) -> RequestProtocol {
    // GET requests: check query param for encoding
    if *req.method() == Method::GET {
        if let Some(query) = req.uri().query() {
            // Parse the encoding parameter
            // Query format: ?connect=v1&encoding=proto&message=...&base64=1
            for pair in query.split('&') {
                if let Some(value) = pair.strip_prefix("encoding=") {
                    return if value == "proto" {
                        RequestProtocol::ConnectUnaryProto
                    } else {
                        RequestProtocol::ConnectUnaryJson
                    };
                }
            }
        }
        // GET without encoding param defaults to JSON
        return RequestProtocol::ConnectUnaryJson;
    }

    // POST requests: check Content-Type header
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    RequestProtocol::from_content_type(content_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn test_detect_protocol_post_json() {
        let req = Request::builder()
            .method(Method::POST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryJson);
    }

    #[test]
    fn test_detect_protocol_post_proto() {
        let req = Request::builder()
            .method(Method::POST)
            .header(header::CONTENT_TYPE, "application/proto")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryProto);
    }

    #[test]
    fn test_detect_protocol_post_connect_stream_json() {
        let req = Request::builder()
            .method(Method::POST)
            .header(header::CONTENT_TYPE, "application/connect+json")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectStreamJson);
    }

    #[test]
    fn test_detect_protocol_get_json() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=abc")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryJson);
    }

    #[test]
    fn test_detect_protocol_get_proto() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=proto&message=abc&base64=1")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryProto);
    }

    #[test]
    fn test_detect_protocol_get_no_encoding() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&message=abc")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryJson);
    }
}
