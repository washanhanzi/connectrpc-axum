//! Middleware layer for Connect RPC protocol handling.
//!
//! The [`ConnectLayer`] middleware detects the protocol variant from incoming requests
//! and stores it in request extensions so that response encoding can match the request format.

mod content_type;
mod protocol;
mod protocol_version;
mod timeout;

pub(crate) use content_type::{validate_streaming_content_type, validate_unary_content_type};

use crate::context::MessageLimits;
use crate::error::{Code, ConnectError};
use axum::http::{Method, Request};
use axum::response::Response;
use std::time::Duration;
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
    /// Server-side maximum timeout. If set, the effective timeout is
    /// min(server_timeout, client_timeout) where client_timeout comes from
    /// the Connect-Timeout-Ms header.
    server_timeout: Option<Duration>,
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
            server_timeout: None,
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

    /// Set the server-side maximum timeout.
    ///
    /// When set, the effective timeout for each request is the minimum of:
    /// - This server timeout
    /// - The client's `Connect-Timeout-Ms` header (if present)
    ///
    /// This ensures the smaller timeout always wins, matching Connect-Go's behavior.
    /// On timeout, a Connect `deadline_exceeded` error is returned.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    /// use connectrpc_axum::ConnectLayer;
    ///
    /// let layer = ConnectLayer::new()
    ///     .timeout(Duration::from_secs(30));
    /// ```
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.server_timeout = Some(timeout);
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
            server_timeout: self.server_timeout,
        }
    }
}

/// Service wrapper that provides per-request protocol context and message limits.
#[derive(Debug, Clone)]
pub struct ConnectService<S> {
    inner: S,
    limits: MessageLimits,
    require_protocol_header: bool,
    server_timeout: Option<Duration>,
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
        let protocol = protocol::detect_protocol(&req);
        req.extensions_mut().insert(protocol);

        // Validate for POST requests:
        // - Content-Type is a known Connect protocol type
        // - Protocol version header (if configured)
        // GET requests use ?connect=v1 query param, validated in request.rs
        if *req.method() == Method::POST {
            // Validate known content-type
            if let Some(err) = content_type::validate_content_type(protocol) {
                let response = err.into_response_with_protocol(protocol);
                return Box::pin(async move { Ok(response) });
            }

            // Validate protocol version header
            if let Some(err) =
                protocol_version::validate_protocol_version(&req, self.require_protocol_header)
            {
                // Return error response immediately without calling inner service
                let response = err.into_response_with_protocol(protocol);
                return Box::pin(async move { Ok(response) });
            }
        }

        // Store message limits in extensions for request extractors
        req.extensions_mut().insert(self.limits);

        // Parse Connect-Timeout-Ms header and compute effective timeout
        let client_timeout = timeout::parse_timeout(&req);
        let effective_timeout =
            timeout::compute_effective_timeout(self.server_timeout, client_timeout);

        // Store effective timeout in extensions (for handlers that need to know)
        // req.extensions_mut().insert(effective_timeout);

        // Clone inner service for the async block
        let inner = self.inner.clone();
        // Replace self.inner with the clone so it's ready for the next request
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move {
            // Apply timeout if configured
            match effective_timeout.duration() {
                Some(duration) => {
                    match tokio::time::timeout(duration, inner.oneshot(req)).await {
                        Ok(result) => result,
                        Err(_elapsed) => {
                            // Timeout exceeded - return Connect deadline_exceeded error
                            let err = ConnectError::new(
                                Code::DeadlineExceeded,
                                "request timeout exceeded",
                            );
                            Ok(err.into_response_with_protocol(protocol))
                        }
                    }
                }
                None => inner.oneshot(req).await,
            }
        })
    }
}
