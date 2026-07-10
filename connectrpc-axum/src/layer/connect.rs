//! Connect protocol middleware layer.
//!
//! The [`ConnectLayer`] middleware detects the protocol variant from incoming requests,
//! builds a [`ConnectContext`], and stores it in request extensions for use by pipelines.

use crate::context::error::ProtocolNegotiationError;
use crate::context::protocol::{can_handle_content_type, can_handle_get_encoding, detect_protocol};
use crate::context::{
    CompressionConfig, ConnectContext, ConnectTimeout, MessageLimits, ServerConfig,
};
use crate::message::error::{Code, ConnectError, build_end_stream_frame_with_limit};
use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use connectrpc_axum_core::{ENVELOPE_HEADER_SIZE, envelope_flags, parse_envelope_header};
use futures::StreamExt;
use http_body::Frame;
use http_body_util::BodyStream;
use std::time::Duration;
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll},
};
use tokio::time::{Instant, timeout_at};
use tower::{Layer, Service, ServiceExt};

/// Layer that wraps services with Connect protocol detection and message limits.
///
/// This layer:
/// 1. Detects the protocol variant from the request (Content-Type header or query params)
/// 2. Validates protocol version header (if configured)
/// 3. Builds a [`ConnectContext`] with protocol, limits, compression, and timeout
/// 4. Stores the context in request extensions for use by request/response pipelines
///
/// # Example
///
/// ```rust,ignore
/// use connectrpc_axum::{ConnectLayer, MessageLimits};
///
/// // Use default (no message size limits)
/// let router = Router::new()
///     .route("/service/Method", post(handler))
///     .layer(ConnectLayer::new());
///
/// // Custom 16 MB receive limit with protocol header required
/// let router = Router::new()
///     .route("/service/Method", post(handler))
///     .layer(
///         ConnectLayer::new()
///             .limits(MessageLimits::new().receive_max_bytes(16 * 1024 * 1024))
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
    /// Create a new ConnectLayer with default settings (no message size limits).
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
    /// This ensures the smaller timeout always wins, matching Connect-Go's timeout
    /// selection. On timeout, a Connect `deadline_exceeded` error is returned.
    ///
    /// Handler execution and streaming response bodies share one absolute deadline.
    /// If that deadline expires after streaming response headers are produced, the
    /// response body ends with a Connect `deadline_exceeded` EndStream frame.
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
    /// - `min_bytes`: Minimum response size before compression is applied (default: 0, matching connect-go)
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

// ============================================================================
// Pre-protocol validation
// ============================================================================

/// Check if the request can be handled by the Connect protocol.
///
/// Returns `Some(ProtocolNegotiationError)` if the request cannot be handled,
/// which should result in an HTTP 415 response with `Accept-Post` header.
///
/// This is called before context creation to handle cases where the protocol
/// cannot be determined (unsupported content-type or invalid GET encoding).
fn check_protocol_negotiation<B>(req: &Request<B>) -> Option<ProtocolNegotiationError> {
    if *req.method() == Method::GET {
        // For GET requests, check the encoding parameter
        if !can_handle_get_encoding(req) {
            return Some(ProtocolNegotiationError::UnsupportedMediaType);
        }
    } else if *req.method() == Method::POST {
        // For POST requests, check the Content-Type
        let protocol = detect_protocol(req);
        if !can_handle_content_type(protocol) {
            return Some(ProtocolNegotiationError::UnsupportedMediaType);
        }
    }
    None
}

// ============================================================================
// ConnectService
// ============================================================================

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
        // 0. Pre-protocol validation (can produce HTTP 415)
        if let Some(nego_err) = check_protocol_negotiation(&req) {
            let response = nego_err.into_response();
            return Box::pin(async move { Ok(response) });
        }

        // 1. Build request context from request headers
        let request_ctx = match ConnectContext::from_request(&req, &self.config) {
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
        let deadline = timeout.map(|duration| Instant::now() + duration);
        let protocol = request_ctx.protocol;
        let send_max_bytes = request_ctx.limits.get_send_max_bytes();

        // 4. Store context in request extensions
        req.extensions_mut().insert(request_ctx);
        req.extensions_mut()
            .insert(ConnectTimeout::from_duration(timeout));

        // Clone inner service for the async block
        let inner = self.inner.clone();
        // Replace self.inner with the clone so it's ready for the next request
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move {
            let response = match deadline {
                Some(deadline) => match timeout_at(deadline, inner.oneshot(req)).await {
                    Ok(result) => result?,
                    Err(_elapsed) => {
                        let err =
                            ConnectError::new(Code::DeadlineExceeded, "request timeout exceeded");
                        return Ok(err.into_response_with_send_limit(protocol, send_max_bytes));
                    }
                },
                None => inner.oneshot(req).await?,
            };

            let is_connect_streaming_response = response.status() == StatusCode::OK
                && response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.split(';').next())
                    .is_some_and(|value| {
                        matches!(
                            value.trim(),
                            "application/connect+json" | "application/connect+proto"
                        )
                    });

            let Some(deadline) = deadline.filter(|_| is_connect_streaming_response) else {
                return Ok(response);
            };

            let (parts, body) = response.into_parts();
            let deadline_error =
                ConnectError::new(Code::DeadlineExceeded, "request timeout exceeded");
            let deadline_frame = Bytes::from(build_end_stream_frame_with_limit(
                Some(&deadline_error),
                None,
                send_max_bytes,
            ));
            let response_frames = async_stream::stream! {
                let mut frames = BodyStream::new(body);
                let sleep = tokio::time::sleep_until(deadline);
                tokio::pin!(sleep);

                loop {
                    tokio::select! {
                        biased;
                        _ = &mut sleep => {
                            yield Ok::<_, axum::Error>(Frame::data(deadline_frame));
                            break;
                        }
                        frame = frames.next() => {
                            let Some(frame) = frame else {
                                break;
                            };
                            let body_error = frame.is_err();
                            let contains_end_stream = frame
                                .as_ref()
                                .ok()
                                .and_then(Frame::data_ref)
                                .is_some_and(|data| {
                                    let mut offset = 0;
                                    while data.len().saturating_sub(offset) >= ENVELOPE_HEADER_SIZE {
                                        let Ok((flags, length)) = parse_envelope_header(&data[offset..])
                                        else {
                                            return false;
                                        };
                                        let Some(frame_end) = offset
                                            .checked_add(ENVELOPE_HEADER_SIZE)
                                            .and_then(|payload_start| payload_start.checked_add(length as usize))
                                        else {
                                            return false;
                                        };
                                        if frame_end > data.len() {
                                            return false;
                                        }
                                        if flags & envelope_flags::END_STREAM != 0 {
                                            return true;
                                        }
                                        offset = frame_end;
                                    }
                                    false
                                });
                            yield frame;
                            if body_error || contains_end_stream {
                                break;
                            }
                        }
                    }
                }
            };

            Ok(Response::from_parts(
                parts,
                Body::new(http_body_util::StreamBody::new(response_frames)),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn ok_service(_req: Request<Body>) -> Result<Response, std::convert::Infallible> {
        Ok(Response::new(Body::empty()))
    }

    fn connect_request(timeout: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .header("content-type", "application/json");
        if let Some(timeout) = timeout {
            builder = builder.header("connect-timeout-ms", timeout);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn malformed_timeout_header_returns_invalid_argument() {
        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(ok_service);

        let resp = svc.oneshot(connect_request(Some("abc"))).await.unwrap();
        // InvalidArgument maps to HTTP 400 for unary
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn too_long_timeout_header_returns_invalid_argument() {
        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(ok_service);

        let resp = svc
            .oneshot(connect_request(Some("12345678901")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn valid_timeout_header_passes_through() {
        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(ok_service);

        let resp = svc.oneshot(connect_request(Some("5000"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn connect_timeout_extension_inserted() {
        use std::time::Duration;

        async fn assert_extension(
            req: Request<Body>,
        ) -> Result<Response, std::convert::Infallible> {
            let timeout = req
                .extensions()
                .get::<ConnectTimeout>()
                .expect("ConnectTimeout extension should be inserted");
            assert_eq!(timeout.duration(), Some(Duration::from_millis(5000)));
            Ok(Response::new(Body::empty()))
        }

        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(assert_extension);

        let resp = svc.oneshot(connect_request(Some("5000"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn zero_timeout_is_immediately_expired_deadline() {
        async fn slow_service(_req: Request<Body>) -> Result<Response, std::convert::Infallible> {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            Ok(Response::new(Body::empty()))
        }

        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(slow_service);

        let resp = svc.oneshot(connect_request(Some("0"))).await.unwrap();
        // DeadlineExceeded maps to HTTP 504 for unary
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn streaming_response_body_uses_handler_deadline() {
        async fn delayed_streaming_service(
            _req: Request<Body>,
        ) -> Result<Response, std::convert::Infallible> {
            tokio::time::sleep(Duration::from_secs(3)).await;
            Ok(Response::builder()
                .header(header::CONTENT_TYPE, "application/connect+json")
                .body(Body::from_stream(futures::stream::pending::<
                    Result<Bytes, std::convert::Infallible>,
                >()))
                .unwrap())
        }

        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(delayed_streaming_service);
        let started_at = Instant::now();
        let response = svc.oneshot(connect_request(Some("5000"))).await.unwrap();
        assert_eq!(started_at.elapsed(), Duration::from_secs(3));

        let body = response.into_body().collect();
        tokio::pin!(body);
        assert!(futures::poll!(body.as_mut()).is_pending());
        tokio::time::advance(Duration::from_millis(1999)).await;
        assert!(futures::poll!(body.as_mut()).is_pending());
        tokio::time::advance(Duration::from_millis(1)).await;

        let body = body.await.unwrap().to_bytes();
        assert_eq!(started_at.elapsed(), Duration::from_secs(5));
        assert_eq!(body[0], envelope_flags::END_STREAM);
        let payload_length = u32::from_be_bytes(body[1..5].try_into().unwrap()) as usize;
        assert_eq!(body.len(), payload_length + 5);
        let payload: serde_json::Value = serde_json::from_slice(&body[5..]).unwrap();
        assert_eq!(payload["error"]["code"], "deadline_exceeded");
        assert_eq!(payload["error"]["message"], "request timeout exceeded");
    }

    #[tokio::test(start_paused = true)]
    async fn completed_streaming_response_does_not_emit_deadline_error() {
        async fn completed_streaming_service(
            _req: Request<Body>,
        ) -> Result<Response, std::convert::Infallible> {
            let frame = build_end_stream_frame_with_limit(None, None, None);
            Ok(Response::builder()
                .header(header::CONTENT_TYPE, "application/connect+proto")
                .body(Body::from(frame))
                .unwrap())
        }

        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(completed_streaming_service);
        let expected = build_end_stream_frame_with_limit(None, None, None);
        let response = svc.oneshot(connect_request(Some("5000"))).await.unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected);
    }
}
