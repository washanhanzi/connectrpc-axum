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
use bytes::BytesMut;
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
                let mut scanner = EndStreamScanner::new();
                let mut partial_envelope = BytesMut::new();

                loop {
                    tokio::select! {
                        biased;
                        _ = &mut sleep => {
                            // Partial envelope bytes have not been forwarded. Dropping
                            // them keeps the deadline EndStream at an envelope boundary.
                            partial_envelope.clear();
                            yield Ok::<_, axum::Error>(Frame::data(deadline_frame));
                            break;
                        }
                        frame = frames.next() => {
                            let Some(frame) = frame else {
                                if !partial_envelope.is_empty() {
                                    yield Ok(Frame::data(partial_envelope.split().freeze()));
                                }
                                break;
                            };

                            let frame = match frame {
                                Ok(frame) => frame,
                                Err(error) => {
                                    if !partial_envelope.is_empty() {
                                        yield Ok(Frame::data(partial_envelope.split().freeze()));
                                    }
                                    yield Err(error);
                                    break;
                                }
                            };
                            let data = match frame.into_data() {
                                Ok(data) => data,
                                Err(frame) => {
                                    if !partial_envelope.is_empty() {
                                        yield Ok(Frame::data(partial_envelope.split().freeze()));
                                    }
                                    yield Ok(frame);
                                    continue;
                                }
                            };

                            let (complete_through, end_stream_done) = scanner.scan(&data);
                            if end_stream_done {
                                if partial_envelope.is_empty() {
                                    yield Ok(Frame::data(data));
                                } else {
                                    partial_envelope.extend_from_slice(&data);
                                    yield Ok(Frame::data(partial_envelope.split().freeze()));
                                }
                                break;
                            }

                            if complete_through == 0 {
                                partial_envelope.extend_from_slice(&data);
                                continue;
                            }

                            let complete_envelopes = if partial_envelope.is_empty() {
                                data.slice(..complete_through)
                            } else {
                                partial_envelope.extend_from_slice(&data[..complete_through]);
                                partial_envelope.split().freeze()
                            };
                            partial_envelope.extend_from_slice(&data[complete_through..]);
                            yield Ok(Frame::data(complete_envelopes));
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

/// Incremental envelope scanner that finds complete envelope boundaries.
///
/// The deadline wrapper needs to know when the response body's EndStream
/// envelope has passed through so it can stop watching the deadline. Body
/// chunks are not guaranteed to align with envelope boundaries: middleware
/// between the handler and [`ConnectLayer`] may re-chunk the body, splitting
/// an envelope across chunks. A stateless per-chunk scan would then
/// misinterpret payload bytes as envelope headers — a payload byte with the
/// END_STREAM bit set (e.g. protobuf tag `0x12`) followed by a plausible
/// length would falsely match, truncating the stream.
///
/// This scanner instead tracks envelope boundaries across chunks: it buffers
/// the 5-byte header (which may itself span chunks), skips exactly `length`
/// payload bytes, and only inspects flag bytes at true envelope offsets. The
/// deadline wrapper withholds bytes after the last complete boundary so a
/// timeout cannot insert an EndStream into an unfinished envelope.
struct EndStreamScanner {
    /// Partially accumulated envelope header.
    header: [u8; ENVELOPE_HEADER_SIZE],
    /// Number of header bytes accumulated so far.
    header_filled: usize,
    /// Payload bytes of the current envelope not yet observed.
    payload_remaining: u64,
    /// Whether the current envelope has the END_STREAM flag set.
    in_end_stream: bool,
}

impl EndStreamScanner {
    fn new() -> Self {
        Self {
            header: [0; ENVELOPE_HEADER_SIZE],
            header_filled: 0,
            payload_remaining: 0,
            in_end_stream: false,
        }
    }

    /// Feeds the next body chunk to the scanner.
    ///
    /// Returns the number of bytes from this chunk through the last complete
    /// envelope and whether that envelope was EndStream. Bytes after the
    /// returned offset belong to an incomplete envelope and must not be
    /// forwarded until a later chunk completes it.
    fn scan(&mut self, mut data: &[u8]) -> (usize, bool) {
        let chunk_len = data.len();
        let mut complete_through = 0;
        loop {
            // Skip payload bytes of the envelope currently being observed.
            if self.payload_remaining > 0 {
                let skip = self.payload_remaining.min(data.len() as u64) as usize;
                data = &data[skip..];
                self.payload_remaining -= skip as u64;
                if self.payload_remaining > 0 {
                    return (complete_through, false);
                }
                complete_through = chunk_len - data.len();
                if self.in_end_stream {
                    return (complete_through, true);
                }
            }
            if data.is_empty() {
                return (complete_through, false);
            }
            // Accumulate the next envelope header, which may span chunks.
            let take = (ENVELOPE_HEADER_SIZE - self.header_filled).min(data.len());
            self.header[self.header_filled..self.header_filled + take]
                .copy_from_slice(&data[..take]);
            self.header_filled += take;
            data = &data[take..];
            if self.header_filled < ENVELOPE_HEADER_SIZE {
                return (complete_through, false);
            }
            let (flags, length) =
                parse_envelope_header(&self.header).expect("header buffer is exactly header-sized");
            self.header_filled = 0;
            self.payload_remaining = u64::from(length);
            self.in_end_stream = flags & envelope_flags::END_STREAM != 0;
            if self.payload_remaining == 0 {
                complete_through = chunk_len - data.len();
                if self.in_end_stream {
                    return (complete_through, true);
                }
            }
        }
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

    fn envelope(flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![flags];
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn scanner_detects_end_stream_in_aligned_chunk() {
        let mut scanner = EndStreamScanner::new();
        let mut chunk = envelope(envelope_flags::MESSAGE, b"hello");
        chunk.extend_from_slice(&envelope(envelope_flags::END_STREAM, b"{}"));
        assert_eq!(scanner.scan(&chunk), (chunk.len(), true));
    }

    #[test]
    fn scanner_ignores_decoy_header_at_chunk_start() {
        // Payload bytes that mimic an empty EndStream envelope header: a flag
        // byte with the END_STREAM bit set followed by a zero length. When
        // re-chunking puts them at the start of a chunk, a stateless scan
        // would falsely detect the end of the stream.
        let mut scanner = EndStreamScanner::new();
        let frame = envelope(envelope_flags::MESSAGE, b"\x02\x00\x00\x00\x00rest");
        assert_eq!(scanner.scan(&frame[..ENVELOPE_HEADER_SIZE]), (0, false));
        assert_eq!(
            scanner.scan(&frame[ENVELOPE_HEADER_SIZE..]),
            (frame.len() - ENVELOPE_HEADER_SIZE, false)
        );
        let end_stream = envelope(envelope_flags::END_STREAM, b"{}");
        assert_eq!(scanner.scan(&end_stream), (end_stream.len(), true));
    }

    #[test]
    fn scanner_detects_end_stream_split_across_chunks() {
        let mut scanner = EndStreamScanner::new();
        let mut bytes = envelope(envelope_flags::MESSAGE, b"hi");
        bytes.extend_from_slice(&envelope(envelope_flags::END_STREAM, b"{}"));
        // Chunks of 3 split both envelope headers and payloads across chunks.
        let mut done = false;
        for chunk in bytes.chunks(3) {
            assert!(!done, "scanner returned true before the final chunk");
            done = scanner.scan(chunk).1;
        }
        assert!(done, "scanner missed the split EndStream envelope");
    }

    #[tokio::test]
    async fn rechunked_streaming_response_passes_through_intact() {
        // Regression test: middleware between the handler and ConnectLayer may
        // re-chunk the body so envelopes span chunks. With a timeout armed,
        // the deadline wrapper must not mistake payload bytes at a chunk start
        // for an EndStream header and truncate the stream.
        let mut body_bytes = envelope(envelope_flags::MESSAGE, b"\x02\x00\x00\x00\x00payload");
        body_bytes.extend_from_slice(&envelope(envelope_flags::MESSAGE, b"second"));
        body_bytes.extend_from_slice(&build_end_stream_frame_with_limit(None, None, None));
        let expected = body_bytes.clone();

        // 5-byte chunks: the second chunk starts with the decoy bytes
        // \x02\x00\x00\x00\x00, which mimic an empty EndStream envelope.
        let chunks: Vec<Result<Bytes, std::convert::Infallible>> = body_bytes
            .chunks(ENVELOPE_HEADER_SIZE)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();

        let rechunked_streaming_service = move |_req: Request<Body>| {
            let chunks = chunks.clone();
            async move {
                Ok::<_, std::convert::Infallible>(
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/connect+proto")
                        .body(Body::from_stream(futures::stream::iter(chunks)))
                        .unwrap(),
                )
            }
        };

        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(rechunked_streaming_service);
        let response = svc.oneshot(connect_request(Some("60000"))).await.unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected);
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_replaces_partial_envelope_at_frame_boundary() {
        // The first body chunk contains one complete envelope followed by a
        // partial header. If the deadline expires before the rest arrives, the
        // wrapper must discard only the partial envelope and append the
        // deadline EndStream after the complete envelope.
        let first_envelope = envelope(envelope_flags::MESSAGE, b"first");
        let second_envelope = envelope(envelope_flags::MESSAGE, b"second");
        let mut first_chunk = first_envelope.clone();
        first_chunk.extend_from_slice(&second_envelope[..3]);
        let second_chunk = Bytes::copy_from_slice(&second_envelope[3..]);
        let success_end_stream = Bytes::from(build_end_stream_frame_with_limit(None, None, None));

        let streaming_service = move |_req: Request<Body>| {
            let first_chunk = Bytes::from(first_chunk.clone());
            let second_chunk = second_chunk.clone();
            let success_end_stream = success_end_stream.clone();
            async move {
                let body = async_stream::stream! {
                    yield Ok::<_, std::convert::Infallible>(first_chunk);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    yield Ok(second_chunk);
                    yield Ok(success_end_stream);
                };
                Ok::<_, std::convert::Infallible>(
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/connect+proto")
                        .body(Body::from_stream(body))
                        .unwrap(),
                )
            }
        };

        let svc = tower::ServiceBuilder::new()
            .layer(ConnectLayer::new())
            .service_fn(streaming_service);
        let response = svc.oneshot(connect_request(Some("1000"))).await.unwrap();

        let deadline_error = ConnectError::new(Code::DeadlineExceeded, "request timeout exceeded");
        let mut expected = first_envelope;
        expected.extend_from_slice(&build_end_stream_frame_with_limit(
            Some(&deadline_error),
            None,
            None,
        ));

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected);
    }
}
