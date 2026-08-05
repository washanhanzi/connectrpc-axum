//! Compression bridge layer for Connect protocol.
//!
//! See the [parent module](super) documentation for details on why this layer exists.

use axum::body::Body;
use axum::http::header::{
    ACCEPT_ENCODING, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, EXPECT,
};
use axum::http::{HeaderValue, Request, Version};
use axum::response::Response;
use http_body_util::BodyExt;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service, ServiceExt};

use crate::context::MessageLimits;
use crate::context::protocol::detect_protocol;
use crate::message::error::{Code, ConnectError};

/// Layer that bridges Tower compression with Connect protocol requirements.
///
/// For streaming requests, sets `Accept-Encoding: identity` to prevent Tower
/// from compressing the response (streaming uses per-envelope compression).
///
/// Also enforces unary request body size limits on the compressed body before decompression.
/// Streaming limits apply independently to each envelope.
///
/// This is algorithm-agnostic and works with any compression layer inside.
///
/// # Example
///
/// ```rust,ignore
/// use tower_http::compression::CompressionLayer;
/// use connectrpc_axum::{ConnectLayer, BridgeLayer};
///
/// let app = Router::new()
///     .route("/service/Method", post(handler))
///     .layer(ConnectLayer::new())
///     .layer(CompressionLayer::new())
///     .layer(BridgeLayer::new());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct BridgeLayer {
    /// Maximum request body size in bytes (compressed size).
    /// `None` means unlimited.
    receive_max_bytes: Option<usize>,
    /// Maximum size for streaming error control frames.
    send_max_bytes: Option<usize>,
}

impl BridgeLayer {
    /// Create a new BridgeLayer with no size limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new BridgeLayer with the specified receive size limit.
    ///
    /// This limits the compressed unary request body size (from `Content-Length` header).
    /// Oversized HTTP/1 request bodies are drained without buffering so clients can
    /// receive the error response. Rejection occurs before decompression.
    ///
    /// Use `None` for unlimited (not recommended for production).
    pub fn with_receive_limit(receive_max_bytes: Option<usize>) -> Self {
        Self {
            receive_max_bytes,
            send_max_bytes: None,
        }
    }

    /// Create a new BridgeLayer with the specified message limits.
    pub fn with_limits(limits: MessageLimits) -> Self {
        Self {
            receive_max_bytes: limits.get_receive_max_bytes(),
            send_max_bytes: limits.get_send_max_bytes(),
        }
    }
}

impl<S> Layer<S> for BridgeLayer {
    type Service = BridgeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        BridgeService {
            inner,
            receive_max_bytes: self.receive_max_bytes,
            send_max_bytes: self.send_max_bytes,
        }
    }
}

/// Service that bridges Tower compression with Connect protocol.
#[derive(Debug, Clone)]
pub struct BridgeService<S> {
    inner: S,
    receive_max_bytes: Option<usize>,
    send_max_bytes: Option<usize>,
}

impl<S> Service<Request<Body>> for BridgeService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let connect_streaming = is_connect_streaming(&req);

        // Streaming Content-Length covers every envelope, while the configured
        // receive limit applies to each message independently.
        if !connect_streaming
            && let Some(max_size) = self.receive_max_bytes
            && let Some(content_length) = get_content_length(&req)
            && content_length > max_size
        {
            // Detect protocol to return proper error format
            let protocol = detect_protocol(&req);
            let err = ConnectError::new(
                Code::ResourceExhausted,
                format!(
                    "request body size {} exceeds maximum allowed size of {} bytes",
                    content_length, max_size
                ),
            );
            let send_max_bytes = self.send_max_bytes;
            let is_http1 = matches!(
                req.version(),
                Version::HTTP_09 | Version::HTTP_10 | Version::HTTP_11
            );
            let close_connection = is_http1 && expects_continue(&req);
            let body = (is_http1 && !close_connection).then(|| req.into_body());
            return Box::pin(async move {
                if let Some(body) = body {
                    drain_body(body).await;
                }
                let mut response = err.into_response_with_send_limit(protocol, send_max_bytes);
                if close_connection {
                    response
                        .headers_mut()
                        .insert(CONNECTION, HeaderValue::from_static("close"));
                }
                Ok(response)
            });
        }

        if connect_streaming {
            // Streaming uses per-envelope compression via Connect-Content-Encoding/Connect-Accept-Encoding,
            // NOT HTTP body compression via Content-Encoding/Accept-Encoding.
            // Remove Content-Encoding to prevent Tower from decompressing the request body.
            req.headers_mut().remove(CONTENT_ENCODING);
            // Set Accept-Encoding: identity to prevent Tower from compressing the response body.
            req.headers_mut()
                .insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        }

        // Clone inner service for the async block
        let inner = self.inner.clone();
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move { inner.oneshot(req).await })
    }
}

/// Drain an HTTP/1 request body so the client can finish writing and receive the RPC error.
async fn drain_body(mut body: Body) {
    while let Some(frame) = body.frame().await {
        if frame.is_err() {
            break;
        }
    }
}

/// Check whether an HTTP/1 client is waiting for permission to send the request body.
fn expects_continue<B>(req: &Request<B>) -> bool {
    req.headers()
        .get_all(EXPECT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|expectation| expectation.trim().eq_ignore_ascii_case("100-continue"))
}

/// Get Content-Length header value as usize.
fn get_content_length<B>(req: &Request<B>) -> Option<usize> {
    req.headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

/// Check if the request is a Connect streaming request.
///
/// Connect streaming uses content types like:
/// - `application/connect+json`
/// - `application/connect+proto`
fn is_connect_streaming<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("application/connect+"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use bytes::Bytes;
    use futures::StreamExt;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tower::{ServiceBuilder, ServiceExt};

    // Simple echo service for testing
    async fn echo_service(req: Request<Body>) -> Result<Response, std::convert::Infallible> {
        let accept = req
            .headers()
            .get(ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("none")
            .to_string();

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("x-accept-encoding", accept)
            .body(Body::empty())
            .unwrap())
    }

    fn counted_body(chunks: Vec<Bytes>) -> (Body, Arc<AtomicUsize>) {
        let chunks_read = Arc::new(AtomicUsize::new(0));
        let stream =
            futures::stream::iter(chunks.into_iter().map(Ok::<_, std::convert::Infallible>))
                .inspect({
                    let chunks_read = Arc::clone(&chunks_read);
                    move |_| {
                        chunks_read.fetch_add(1, Ordering::Relaxed);
                    }
                });
        (Body::from_stream(stream), chunks_read)
    }

    #[tokio::test]
    async fn test_unary_request_passes_through() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::new())
            .service_fn(echo_service);

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT_ENCODING, "gzip")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Accept-Encoding should NOT be modified for unary
        assert_eq!(
            resp.headers()
                .get("x-accept-encoding")
                .unwrap()
                .to_str()
                .unwrap(),
            "gzip"
        );
    }

    #[tokio::test]
    async fn test_streaming_request_overrides_accept_encoding() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::new())
            .service_fn(echo_service);

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/connect+proto")
            .header(ACCEPT_ENCODING, "gzip")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Accept-Encoding should be overridden to identity for streaming
        assert_eq!(
            resp.headers()
                .get("x-accept-encoding")
                .unwrap()
                .to_str()
                .unwrap(),
            "identity"
        );
    }

    #[test]
    fn test_is_connect_streaming() {
        // Streaming content types
        let req = Request::builder()
            .header(CONTENT_TYPE, "application/connect+json")
            .body(())
            .unwrap();
        assert!(is_connect_streaming(&req));

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/connect+proto")
            .body(())
            .unwrap();
        assert!(is_connect_streaming(&req));

        // Unary content types
        let req = Request::builder()
            .header(CONTENT_TYPE, "application/json")
            .body(())
            .unwrap();
        assert!(!is_connect_streaming(&req));

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/proto")
            .body(())
            .unwrap();
        assert!(!is_connect_streaming(&req));

        // No content type
        let req = Request::builder().body(()).unwrap();
        assert!(!is_connect_streaming(&req));
    }

    #[tokio::test]
    async fn test_size_limit_within_limit() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(1000)))
            .service_fn(echo_service);

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "500")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_size_limit_exceeds_limit() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(1000)))
            .service_fn(echo_service);

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "2000")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        // ResourceExhausted maps to 429 Too Many Requests in Connect protocol
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn oversized_http1_unary_body_is_drained_before_response() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(4)))
            .service_fn(echo_service);
        let (body, chunks_read) =
            counted_body(vec![Bytes::from_static(b"abc"), Bytes::from_static(b"def")]);
        let req = Request::builder()
            .version(Version::HTTP_11)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "6")
            .body(body)
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(chunks_read.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn oversized_http2_unary_body_is_rejected_without_draining() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(4)))
            .service_fn(echo_service);
        let (body, chunks_read) = counted_body(vec![Bytes::from_static(b"abcdef")]);
        let req = Request::builder()
            .version(Version::HTTP_2)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "6")
            .body(body)
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(chunks_read.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn oversized_expect_continue_body_is_rejected_without_draining() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(4)))
            .service_fn(echo_service);
        let (body, chunks_read) = counted_body(vec![Bytes::from_static(b"abcdef")]);
        let req = Request::builder()
            .version(Version::HTTP_11)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "6")
            .header(EXPECT, "100-continue")
            .body(body)
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get(CONNECTION).unwrap(), "close");
        assert_eq!(chunks_read.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn streaming_content_length_is_not_compared_to_per_message_limit() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(64)))
            .service_fn(echo_service);

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/connect+json")
            .header(CONTENT_LENGTH, "138")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_size_limit_no_content_length() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(Some(1000)))
            .service_fn(echo_service);

        // No Content-Length header - should pass through
        let req = Request::builder()
            .header(CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_no_size_limit() {
        let svc = ServiceBuilder::new()
            .layer(BridgeLayer::with_receive_limit(None)) // Unlimited
            .service_fn(echo_service);

        let req = Request::builder()
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "999999999")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
