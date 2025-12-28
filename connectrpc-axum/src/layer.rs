//! Middleware layer for Connect RPC protocol handling.
//!
//! The [`ConnectLayer`] middleware detects the protocol variant from incoming requests
//! and stores it in request extensions so that response encoding can match the request format.

use crate::error::{Code, ConnectError};
use crate::limits::MessageLimits;
use crate::protocol::RequestProtocol;
use axum::http::{header, Method, Request};
use axum::response::Response;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service, ServiceExt};

/// The expected Connect protocol version.
const CONNECT_PROTOCOL_VERSION: &str = "1";

/// Header name for Connect protocol version.
const CONNECT_PROTOCOL_VERSION_HEADER: &str = "connect-protocol-version";

/// Layer that wraps services with Connect protocol detection and message limits.
///
/// This layer:
/// 1. Detects the protocol variant from the request (Content-Type header or query params)
/// 2. Validates protocol version header (if configured)
/// 3. Stores the [`RequestProtocol`] and [`MessageLimits`] in request extensions
/// 4. Handler wrappers extract the protocol and inject it into response types
/// 5. Request extractors enforce message size limits
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
/// // Custom 16 MB limit with protocol header required
/// let router = Router::new()
///     .route("/service/Method", post(handler))
///     .layer(
///         ConnectLayer::new()
///             .limits(MessageLimits::new(16 * 1024 * 1024))
///             .require_protocol_header(true)
///     );
/// ```
///
#[derive(Debug, Clone, Copy)]
pub struct ConnectLayer {
    limits: MessageLimits,
    require_protocol_header: bool,
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
            require_protocol_header: false,
        }
    }

    /// Set custom message limits.
    pub fn limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Require the `Connect-Protocol-Version` header on Connect protocol requests.
    ///
    /// When enabled, requests must include the `Connect-Protocol-Version: 1` header.
    /// This helps HTTP proxies and middleware identify valid Connect requests.
    ///
    /// Disabled by default to allow easy ad-hoc requests (e.g., with cURL).
    pub fn require_protocol_header(mut self, require: bool) -> Self {
        self.require_protocol_header = require;
        self
    }
}

impl<S> Layer<S> for ConnectLayer {
    type Service = ConnectService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectService {
            inner,
            limits: self.limits,
            require_protocol_header: self.require_protocol_header,
        }
    }
}

/// Service wrapper that provides per-request protocol context and message limits.
#[derive(Debug, Clone)]
pub struct ConnectService<S> {
    inner: S,
    limits: MessageLimits,
    require_protocol_header: bool,
}

impl<S, ReqBody> Service<Request<ReqBody>> for ConnectService<S>
where
    S: Service<Request<ReqBody>, Response = Response> + Clone + Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Detect protocol from request and store in extensions
        let protocol = detect_protocol(&req);
        req.extensions_mut().insert(protocol);

        // Validate protocol version for POST requests
        // GET requests use ?connect=v1 query param, validated in request.rs
        if *req.method() == Method::POST {
            if let Some(err) = validate_protocol_version(&req, self.require_protocol_header) {
                // Return error response immediately without calling inner service
                let response = err.into_response_with_protocol(protocol);
                return Box::pin(async move { Ok(response) });
            }
        }

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

/// Validate the Connect-Protocol-Version header for POST requests.
///
/// Returns `Some(ConnectError)` if validation fails, `None` if valid.
///
/// When `require_header` is false (default), the header is optional but if present must be "1".
/// When `require_header` is true, the header is required and must be "1".
fn validate_protocol_version<B>(req: &Request<B>, require_header: bool) -> Option<ConnectError> {
    let version = req
        .headers()
        .get(CONNECT_PROTOCOL_VERSION_HEADER)
        .and_then(|v| v.to_str().ok());

    match version {
        // Header present with correct value
        Some(v) if v == CONNECT_PROTOCOL_VERSION => None,
        // Header present with wrong value
        Some(v) => Some(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "connect-protocol-version must be \"{}\": got \"{}\"",
                CONNECT_PROTOCOL_VERSION, v
            ),
        )),
        // Header not present
        None if require_header => Some(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "missing required header: set {} to \"{}\"",
                CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION
            ),
        )),
        None => None,
    }
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

    #[test]
    fn test_validate_protocol_version_valid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, "1")
            .body(())
            .unwrap();
        assert!(validate_protocol_version(&req, false).is_none());
        assert!(validate_protocol_version(&req, true).is_none());
    }

    #[test]
    fn test_validate_protocol_version_missing_not_required() {
        // When require_header=false, missing header is OK
        let req = Request::builder()
            .method(Method::POST)
            .body(())
            .unwrap();
        assert!(validate_protocol_version(&req, false).is_none());
    }

    #[test]
    fn test_validate_protocol_version_missing_required() {
        // When require_header=true, missing header is an error
        let req = Request::builder()
            .method(Method::POST)
            .body(())
            .unwrap();
        let err = validate_protocol_version(&req, true);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err.message().unwrap().contains("missing required header"));
    }

    #[test]
    fn test_validate_protocol_version_wrong_version() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, "2")
            .body(())
            .unwrap();
        // Wrong version is an error regardless of require_header setting
        let err = validate_protocol_version(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err.message().unwrap().contains("connect-protocol-version must be"));
    }
}
