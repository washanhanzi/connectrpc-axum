//! Middleware layer for Connect RPC protocol handling.
//!
//! The [`ConnectLayer`] middleware detects the protocol variant from incoming requests,
//! builds a [`Context`], and stores it in request extensions for use by pipelines.

use crate::context::{CompressionConfig, Context, MessageLimits, ServerConfig};
use crate::error::{Code, ConnectError};
use axum::http::Request;
use axum::response::Response;
use std::time::Duration;
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll},
};
use tower::{Layer, Service, ServiceExt};

/// Layer that wraps services with Connect protocol detection and message limits.
///
/// This layer:
/// 1. Detects the protocol variant from the request (Content-Type header or query params)
/// 2. Validates protocol version header (if configured)
/// 3. Builds a [`Context`] with protocol, limits, compression, and timeout
/// 4. Stores the context in request extensions for use by request/response pipelines
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
    config: ServerConfig,
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
            config: ServerConfig::default(),
        }
    }

    /// Set custom message limits.
    pub fn limits(mut self, limits: MessageLimits) -> Self {
        self.config.limits = limits;
        self
    }

    /// Require the `Connect-Protocol-Version` header on Connect protocol requests.
    ///
    /// When enabled, requests must include the `Connect-Protocol-Version: 1` header.
    /// This helps HTTP proxies and middleware identify valid Connect requests.
    ///
    /// Disabled by default to allow easy ad-hoc requests (e.g., with cURL).
    pub fn require_protocol_header(mut self, require: bool) -> Self {
        self.config.require_protocol_header = require;
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
        self.config.server_timeout = Some(timeout);
        self
    }

    /// Set compression configuration.
    ///
    /// Controls response compression behavior:
    /// - `min_bytes`: Minimum response size before compression is applied (default: 1024)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::{ConnectLayer, CompressionConfig};
    ///
    /// // Compress responses >= 512 bytes
    /// let layer = ConnectLayer::new()
    ///     .compression(CompressionConfig::new(512));
    ///
    /// // Disable compression entirely
    /// let layer = ConnectLayer::new()
    ///     .compression(CompressionConfig::disabled());
    /// ```
    pub fn compression(mut self, config: CompressionConfig) -> Self {
        self.config.compression = config;
        self
    }
}

impl<S> Layer<S> for ConnectLayer {
    type Service = ConnectService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectService {
            inner,
            config: self.config,
        }
    }
}

/// Service wrapper that provides per-request protocol context and message limits.
#[derive(Debug, Clone)]
pub struct ConnectService<S> {
    inner: S,
    config: ServerConfig,
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

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // 1. Build request context from request headers
        let request_ctx = match Context::from_request(&req, &self.config) {
            Ok(ctx) => ctx,
            Err(err) => {
                let response = err.into_response();
                return Box::pin(async move { Ok(response) });
            }
        };

        // 2. Validate protocol requirements
        if let Err(err) = request_ctx.validate(&req) {
            let response = err.into_response();
            return Box::pin(async move { Ok(response) });
        }

        // 3. Extract values needed for async block before moving context
        let timeout = request_ctx.timeout;
        let protocol = request_ctx.protocol;

        // 4. Store context in request extensions
        req.extensions_mut().insert(request_ctx);

        // Clone inner service for the async block
        let inner = self.inner.clone();
        // Replace self.inner with the clone so it's ready for the next request
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move {
            // Apply timeout if configured
            match timeout {
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
