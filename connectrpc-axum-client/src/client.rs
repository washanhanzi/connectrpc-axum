//! Connect RPC client implementation.
//!
//! This module provides the main [`ConnectClient`] type for making RPC calls.

use bytes::Bytes;
use http::{Method, Request, header};
use http_body_util::BodyExt;
use tokio::time::timeout;

use connectrpc_axum_core::{Code, CompressionConfig, CompressionEncoding, wrap_envelope};
#[cfg(feature = "tracing")]
use tracing::info_span;

use crate::ClientError;
use crate::config::{
    CallOptions, InterceptorInternal, RequestContext, ResponseContext,
    duration_to_timeout_header,
};
use crate::transport::{HyperTransport, TransportBody};
use futures::{Stream, StreamExt};
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

use crate::builder::ClientBuilder;
use crate::request::FrameEncoder;
use crate::response::error_parser::parse_error_response;
use crate::response::{ConnectResponse, FrameDecoder, Metadata, Streaming};

/// Header name for Connect protocol version.
const CONNECT_PROTOCOL_VERSION_HEADER: &str = "connect-protocol-version";

/// Connect protocol version.
const CONNECT_PROTOCOL_VERSION: &str = "1";

/// Header name for Connect timeout in milliseconds.
const CONNECT_TIMEOUT_HEADER: &str = "connect-timeout-ms";

/// Check if a header name is reserved by the Connect protocol.
///
/// Reserved headers should not be overwritten by user-provided CallOptions headers.
/// Per connect-go: "Headers beginning with 'Connect-' and 'Grpc-' are reserved."
fn is_reserved_header(name: &http::header::HeaderName) -> bool {
    let name_str = name.as_str();
    // Protocol-specific headers
    name_str.starts_with("connect-")
        || name_str.starts_with("grpc-")
        // Content headers set by the client
        || name_str == "content-type"
        || name_str == "content-encoding"
        || name_str == "accept-encoding"
        || name_str == "content-length"
}

/// Connect RPC client.
///
/// The client is generic over `I`: the interceptor chain type.
/// This defaults to `()` (no interceptors).
///
/// Interceptors are added via:
/// - [`ClientBuilder::with_interceptor`]: Header-level interceptors (simple)
/// - [`ClientBuilder::with_message_interceptor`]: Message-level interceptors (typed access)
///
/// Both are internally wrapped and composed via [`Chain`](crate::config::Chain),
/// enabling zero-cost interceptor composition at compile time.
///
/// Use [`ClientBuilder`] or [`ConnectClient::builder`] to create an instance.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::ConnectClient;
///
/// let client = ConnectClient::builder("http://localhost:3000")
///     .use_proto()
///     .build()?;
///
/// let response = client.call_unary::<MyRequest, MyResponse>(
///     "my.package.MyService/MyMethod",
///     &request,
/// ).await?;
/// ```
#[derive(Debug, Clone)]
pub struct ConnectClient<I = ()> {
    /// HTTP transport.
    transport: HyperTransport,
    /// Base URL for the service.
    base_url: String,
    /// Use protobuf encoding (true) or JSON encoding (false).
    use_proto: bool,
    /// Compression configuration for outgoing requests.
    compression: CompressionConfig,
    /// Compression encoding for outgoing request bodies.
    request_encoding: CompressionEncoding,
    /// Accepted compression encodings for responses.
    accept_encoding: Option<CompressionEncoding>,
    /// Default timeout for RPC calls.
    default_timeout: Option<Duration>,
    /// Unified interceptor chain (compile-time composed).
    interceptor: I,
}

impl ConnectClient<()> {
    /// Create a new ClientBuilder with the given base URL.
    ///
    /// This is a convenience method equivalent to `ClientBuilder::new(base_url)`.
    pub fn builder<S: Into<String>>(base_url: S) -> ClientBuilder<()> {
        ClientBuilder::new(base_url)
    }
}

impl<I: InterceptorInternal> ConnectClient<I> {
    /// Create a new ConnectClient.
    ///
    /// This is called by [`ClientBuilder::build`]. Prefer using the builder API.
    pub(crate) fn new(
        transport: HyperTransport,
        base_url: String,
        use_proto: bool,
        compression: CompressionConfig,
        request_encoding: CompressionEncoding,
        accept_encoding: Option<CompressionEncoding>,
        default_timeout: Option<Duration>,
        interceptor: I,
    ) -> Self {
        Self {
            transport,
            base_url,
            use_proto,
            compression,
            request_encoding,
            accept_encoding,
            default_timeout,
            interceptor,
        }
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check if protobuf encoding is enabled.
    pub fn is_proto(&self) -> bool {
        self.use_proto
    }

    /// Get the encoding name (for tracing/debugging).
    #[cfg_attr(not(feature = "tracing"), allow(dead_code))]
    fn encoding_name(&self) -> &'static str {
        if self.use_proto { "proto" } else { "json" }
    }

    /// Get the content type for unary requests.
    fn unary_content_type(&self) -> &'static str {
        if self.use_proto {
            "application/proto"
        } else {
            "application/json"
        }
    }

    /// Get the content type for streaming requests.
    fn streaming_content_type(&self) -> &'static str {
        if self.use_proto {
            "application/connect+proto"
        } else {
            "application/connect+json"
        }
    }

    /// Encode a message for sending.
    fn encode_message<T>(&self, msg: &T) -> Result<Bytes, ClientError>
    where
        T: Message + Serialize,
    {
        if self.use_proto {
            Ok(Bytes::from(msg.encode_to_vec()))
        } else {
            serde_json::to_vec(msg)
                .map(Bytes::from)
                .map_err(|e| ClientError::Encode(format!("JSON encoding failed: {}", e)))
        }
    }

    /// Decode a message from response bytes.
    fn decode_message<T>(&self, bytes: &[u8]) -> Result<T, ClientError>
    where
        T: Message + DeserializeOwned + Default,
    {
        if self.use_proto {
            T::decode(bytes)
                .map_err(|e| ClientError::Decode(format!("protobuf decoding failed: {}", e)))
        } else {
            serde_json::from_slice(bytes)
                .map_err(|e| ClientError::Decode(format!("JSON decoding failed: {}", e)))
        }
    }

    /// Compress request body if configured.
    fn maybe_compress(&self, body: Bytes) -> Result<(Bytes, bool), ClientError> {
        // Check if compression is enabled and body meets threshold
        if self.request_encoding.is_identity() || self.compression.is_disabled() {
            return Ok((body, false));
        }

        if body.len() < self.compression.min_bytes {
            return Ok((body, false));
        }

        // Get codec for the encoding
        let Some(codec) = self
            .request_encoding
            .codec_with_level(self.compression.level)
        else {
            return Ok((body, false));
        };

        // Compress
        let compressed = codec
            .compress(&body)
            .map_err(|e| ClientError::Encode(format!("compression failed: {}", e)))?;

        Ok((compressed, true))
    }

    /// Make a unary RPC call.
    ///
    /// # Arguments
    ///
    /// * `procedure` - The full procedure path (e.g., "my.package.MyService/MyMethod")
    /// * `request` - The request message
    ///
    /// # Returns
    ///
    /// Returns the response message wrapped in [`ConnectResponse`], which includes
    /// response metadata (headers).
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if:
    /// - The request cannot be encoded
    /// - The HTTP request fails
    /// - The server returns an error response
    /// - The response cannot be decoded
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = client.call_unary::<GetUserRequest, GetUserResponse>(
    ///     "users.v1.UserService/GetUser",
    ///     &GetUserRequest { id: "123".to_string() },
    /// ).await?;
    ///
    /// println!("User: {:?}", response.into_inner());
    /// ```
    pub async fn call_unary<Req, Res>(
        &self,
        procedure: &str,
        request: &Req,
    ) -> Result<ConnectResponse<Res>, ClientError>
    where
        Req: Message + Serialize + Clone + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "unary",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Build headers (before RPC interceptor so it can modify them)
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            self.unary_content_type().parse().unwrap(),
        );
        headers.insert(
            CONNECT_PROTOCOL_VERSION_HEADER,
            CONNECT_PROTOCOL_VERSION.parse().unwrap(),
        );

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(t) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                headers.insert(CONNECT_TIMEOUT_HEADER, timeout_ms.parse().unwrap());
            }
        }

        // 2. Apply interceptor to request
        let mut request = request.clone();
        {
            let mut ctx = RequestContext::new(procedure, &mut headers);
            self.interceptor.intercept_request(&mut ctx, &mut request)?;
        }

        // 3. Encode request body
        let body = self.encode_message(&request)?;

        // 5. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // Add Content-Encoding if compressed
        if compressed {
            headers.insert(
                header::CONTENT_ENCODING,
                self.request_encoding.as_str().parse().unwrap(),
            );
        }

        // 6. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 7. Build HTTP request
        let mut req_builder = Request::builder().method(Method::POST).uri(&url);

        // Copy headers
        for (name, value) in headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = self.accept_encoding {
            req_builder = req_builder.header(header::ACCEPT_ENCODING, accept.as_str());
        }

        // Build request with body
        let req = req_builder
            .body(TransportBody::full(body))
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 8. Send request (with client-side timeout if configured)
        let response = if let Some(t) = self.default_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| ClientError::new(Code::DeadlineExceeded, "client timeout exceeded"))??
        } else {
            self.transport.request(req).await?
        };

        // 9. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(status, &response_headers, body_bytes));
        }

        // 10. Handle response decompression
        let content_encoding = response_headers
            .get(header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
            ClientError::Protocol(format!(
                "unsupported response encoding: {:?}",
                content_encoding
            ))
        })?;

        // 11. Get response body
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| ClientError::Transport(format!("failed to read response body: {}", e)))?
            .to_bytes();

        // Decompress if needed
        let body_bytes = if let Some(codec) = response_encoding.codec() {
            codec
                .decompress(&body_bytes)
                .map_err(|e| ClientError::Decode(format!("decompression failed: {}", e)))?
        } else {
            body_bytes
        };

        // 12. Decode response
        let mut message: Res = self.decode_message(&body_bytes)?;

        // 12. Apply interceptor to response
        {
            let ctx = ResponseContext::new(procedure, &response_headers);
            self.interceptor.intercept_response(&ctx, &mut message)?;
        }

        // 14. Extract metadata
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(message, metadata))
    }

    /// Make a unary RPC call with custom options.
    ///
    /// This is the same as [`call_unary`](Self::call_unary) but allows specifying
    /// per-call options like custom headers and timeout overrides.
    ///
    /// # Arguments
    ///
    /// * `procedure` - The full procedure path (e.g., "my.package.MyService/MyMethod")
    /// * `request` - The request message
    /// * `options` - Per-call options (headers, timeout, etc.)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    /// use std::time::Duration;
    ///
    /// let options = CallOptions::new()
    ///     .timeout(Duration::from_secs(5))
    ///     .header("authorization", "Bearer token123");
    ///
    /// let response = client.call_unary_with_options::<GetUserRequest, GetUserResponse>(
    ///     "users.v1.UserService/GetUser",
    ///     &GetUserRequest { id: "123".to_string() },
    ///     options,
    /// ).await?;
    /// ```
    pub async fn call_unary_with_options<Req, Res>(
        &self,
        procedure: &str,
        request: &Req,
        options: CallOptions,
    ) -> Result<ConnectResponse<Res>, ClientError>
    where
        Req: Message + Serialize + Clone + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "unary",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Build headers (before RPC interceptor so it can modify them)
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            self.unary_content_type().parse().unwrap(),
        );
        headers.insert(
            CONNECT_PROTOCOL_VERSION_HEADER,
            CONNECT_PROTOCOL_VERSION.parse().unwrap(),
        );

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(t) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                headers.insert(CONNECT_TIMEOUT_HEADER, timeout_ms.parse().unwrap());
            }
        }

        // Add custom headers from options (skip reserved protocol headers)
        for (name, value) in options.headers.iter() {
            if !is_reserved_header(name) {
                headers.insert(name.clone(), value.clone());
            }
        }

        // 2. Apply interceptor to request
        let mut request = request.clone();
        {
            let mut ctx = RequestContext::new(procedure, &mut headers);
            self.interceptor.intercept_request(&mut ctx, &mut request)?;
        }

        // 3. Encode request body
        let body = self.encode_message(&request)?;

        // 5. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // Add Content-Encoding if compressed
        if compressed {
            headers.insert(
                header::CONTENT_ENCODING,
                self.request_encoding.as_str().parse().unwrap(),
            );
        }

        // 6. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 7. Build HTTP request
        let mut req_builder = Request::builder().method(Method::POST).uri(&url);

        // Copy headers
        for (name, value) in headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = self.accept_encoding {
            req_builder = req_builder.header(header::ACCEPT_ENCODING, accept.as_str());
        }

        // Build request with body
        let req = req_builder
            .body(TransportBody::full(body))
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 8. Send request (with client-side timeout if configured)
        let response = if let Some(t) = effective_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| ClientError::new(Code::DeadlineExceeded, "client timeout exceeded"))??
        } else {
            self.transport.request(req).await?
        };

        // 9. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(status, &response_headers, body_bytes));
        }

        // 10. Handle response decompression
        let content_encoding = response_headers
            .get(header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
            ClientError::Protocol(format!(
                "unsupported response encoding: {:?}",
                content_encoding
            ))
        })?;

        // 11. Get response body
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| ClientError::Transport(format!("failed to read response body: {}", e)))?
            .to_bytes();

        // Decompress if needed
        let body_bytes = if let Some(codec) = response_encoding.codec() {
            codec
                .decompress(&body_bytes)
                .map_err(|e| ClientError::Decode(format!("decompression failed: {}", e)))?
        } else {
            body_bytes
        };

        // 12. Decode response
        let mut message: Res = self.decode_message(&body_bytes)?;

        // 12. Apply interceptor to response
        {
            let ctx = ResponseContext::new(procedure, &response_headers);
            self.interceptor.intercept_response(&ctx, &mut message)?;
        }

        // 14. Extract metadata
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(message, metadata))
    }

    /// Make a server-streaming RPC call.
    ///
    /// The server sends multiple messages in response to a single request.
    ///
    /// # Arguments
    ///
    /// * `procedure` - The full procedure path (e.g., "my.package.MyService/ServerStream")
    /// * `request` - The request message
    ///
    /// # Returns
    ///
    /// Returns a [`ConnectResponse`] containing a [`Streaming`] that yields
    /// response messages. After the stream is consumed, trailers are available
    /// via `stream.trailers()`.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if:
    /// - The request cannot be encoded
    /// - The HTTP request fails
    /// - The server returns an error status immediately
    ///
    /// Individual stream items may also return errors if:
    /// - A message cannot be decoded
    /// - The server sends an error in the EndStream frame
    /// - The connection is lost
    ///
    /// # Example
    ///
    /// ```ignore
    /// use futures::StreamExt;
    ///
    /// let response = client.call_server_stream::<ListRequest, ListItem>(
    ///     "items.v1.ItemService/ListItems",
    ///     &ListRequest { page_size: 10 },
    /// ).await?;
    ///
    /// let mut stream = response.into_inner();
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(item) => println!("Item: {:?}", item),
    ///         Err(e) => eprintln!("Error: {:?}", e),
    ///     }
    /// }
    ///
    /// // Access trailers after stream is consumed
    /// if let Some(trailers) = stream.trailers() {
    ///     println!("Trailers: {:?}", trailers);
    /// }
    /// ```
    pub async fn call_server_stream<Req, Res>(
        &self,
        procedure: &str,
        request: &Req,
    ) -> Result<
        ConnectResponse<
            Streaming<
                FrameDecoder<
                    impl futures::Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res>,
                    Res,
                >,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize,
        Res: Message + DeserializeOwned + Default,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "server_stream",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Encode request body
        let body = self.encode_message(request)?;

        // 2. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // 3. Wrap in envelope for streaming request
        // Connect streaming protocol requires envelope framing even for single-message requests
        let body = Bytes::from(wrap_envelope(&body, compressed));

        // 4. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 5. Build request with streaming content-type
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(header::CONTENT_TYPE, self.streaming_content_type());

        // Add Connect-Content-Encoding if compressed (streaming uses this header, not Content-Encoding)
        if compressed {
            req_builder =
                req_builder.header("connect-content-encoding", self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(t) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Apply interceptors (header-only for streaming initial request)
        let mut interceptor_headers = http::HeaderMap::new();
        {
            let mut ctx = RequestContext::new(procedure, &mut interceptor_headers);
            // Use a unit placeholder - streaming interceptors use on_stream_send for messages
            self.interceptor.intercept_request(&mut ctx, &mut ())?;
        }
        for (name, value) in interceptor_headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Build request with body
        let req = req_builder
            .body(TransportBody::full(body))
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 5. Send request (with client-side timeout if configured)
        let response = if let Some(t) = self.default_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??
        } else {
            self.transport.request(req).await?
        };

        // 6. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(
                status,
                &response_headers,
                body_bytes,
            ));
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        // For streaming, the encoding is per-frame and signaled via this header
        let content_encoding = response_headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding =
            CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                ClientError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get the streaming body and convert to a byte stream
        let body = response.into_body();
        let byte_stream = body_to_stream(body);

        // 9. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 10. Wrap with Streaming
        let stream_body = Streaming::new(decoder);

        // 11. Extract metadata from initial response headers
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(stream_body, metadata))
    }

    /// Make a server-streaming RPC call with custom options.
    ///
    /// This is the same as [`call_server_stream`](Self::call_server_stream) but allows specifying
    /// per-call options like custom headers and timeout overrides.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    ///
    /// let options = CallOptions::new()
    ///     .header("authorization", "Bearer token123");
    ///
    /// let response = client.call_server_stream_with_options::<ListRequest, ListItem>(
    ///     "items.v1.ItemService/ListItems",
    ///     &ListRequest { page_size: 10 },
    ///     options,
    /// ).await?;
    /// ```
    pub async fn call_server_stream_with_options<Req, Res>(
        &self,
        procedure: &str,
        request: &Req,
        options: CallOptions,
    ) -> Result<
        ConnectResponse<
            Streaming<
                FrameDecoder<
                    impl Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res>,
                    Res,
                >,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize,
        Res: Message + DeserializeOwned + Default,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "server_stream",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Encode request body
        let body = self.encode_message(request)?;

        // 2. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // 3. Wrap in envelope for streaming request
        // Connect streaming protocol requires envelope framing even for single-message requests
        let body = Bytes::from(wrap_envelope(&body, compressed));

        // 4. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 5. Build request with streaming content-type
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(header::CONTENT_TYPE, self.streaming_content_type());

        // Add Connect-Content-Encoding if compressed (streaming uses this header, not Content-Encoding)
        if compressed {
            req_builder =
                req_builder.header("connect-content-encoding", self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(t) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Add custom headers from options (skip reserved protocol headers)
        for (name, value) in options.headers.iter() {
            if !is_reserved_header(name) {
                req_builder = req_builder.header(name, value);
            }
        }

        // Apply interceptors (header-only for streaming initial request)
        let mut interceptor_headers = http::HeaderMap::new();
        {
            let mut ctx = RequestContext::new(procedure, &mut interceptor_headers);
            // Use a unit placeholder - streaming interceptors use on_stream_send for messages
            self.interceptor.intercept_request(&mut ctx, &mut ())?;
        }
        for (name, value) in interceptor_headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Build request with body
        let req = req_builder
            .body(TransportBody::full(body))
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 5. Send request (with client-side timeout if configured)
        let response = if let Some(t) = effective_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??
        } else {
            self.transport.request(req).await?
        };

        // 6. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(
                status,
                &response_headers,
                body_bytes,
            ));
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = response_headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding =
            CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                ClientError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get the streaming body
        let body = response.into_body();
        let byte_stream = body_to_stream(body);

        // 9. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 10. Wrap with Streaming
        let stream_body = Streaming::new(decoder);

        // 11. Extract metadata from initial response headers
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(stream_body, metadata))
    }

    /// Make a client-streaming RPC call.
    ///
    /// The client sends multiple messages and receives a single response.
    ///
    /// # Arguments
    ///
    /// * `procedure` - The full procedure path (e.g., "my.package.MyService/ClientStream")
    /// * `request` - A stream of request messages
    ///
    /// # Returns
    ///
    /// Returns a [`ConnectResponse`] containing the single response message.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if:
    /// - The HTTP request fails
    /// - The server returns an error status
    /// - The response cannot be decoded
    ///
    /// # Example
    ///
    /// ```ignore
    /// use futures::stream;
    ///
    /// let messages = stream::iter(vec![
    ///     Message { content: "first".to_string() },
    ///     Message { content: "second".to_string() },
    ///     Message { content: "third".to_string() },
    /// ]);
    ///
    /// let response = client.call_client_stream::<Message, Response>(
    ///     "chat.v1.ChatService/SendMessages",
    ///     messages,
    /// ).await?;
    ///
    /// println!("Response: {:?}", response.into_inner());
    /// ```
    pub async fn call_client_stream<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
    ) -> Result<ConnectResponse<Res>, ClientError>
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "client_stream",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        // 3. Create streaming body from encoder
        let body = TransportBody::streaming(encoder);

        // 4. Build request with streaming content-type
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder =
                req_builder.header("connect-content-encoding", self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(t) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Apply interceptors (header-only for streaming initial request)
        let mut interceptor_headers = http::HeaderMap::new();
        {
            let mut ctx = RequestContext::new(procedure, &mut interceptor_headers);
            // Use a unit placeholder - streaming interceptors use on_stream_send for messages
            self.interceptor.intercept_request(&mut ctx, &mut ())?;
        }
        for (name, value) in interceptor_headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Build request with body
        let req = req_builder
            .body(body)
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 5. Send request (with client-side timeout if configured)
        let response = if let Some(t) = self.default_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??
        } else {
            self.transport.request(req).await?
        };

        // 6. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(
                status,
                &response_headers,
                body_bytes,
            ));
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = response_headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding =
            CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                ClientError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get the streaming body and decode the single response message
        let body = response.into_body();
        let byte_stream = body_to_stream(body);
        let mut decoder =
            FrameDecoder::<_, Res>::new(byte_stream, self.use_proto, response_encoding);

        // 9. Get the single response message
        let message = decoder.next().await.ok_or_else(|| {
            ClientError::Protocol("expected response message but stream ended".into())
        })??;

        // 10. Consume the EndStream frame and check for errors
        // The decoder will return an error if the EndStream frame contains an error
        if let Some(result) = decoder.next().await {
            match result {
                Err(e) => {
                    // EndStream contained an error - propagate it
                    return Err(e);
                }
                Ok(_) => {
                    // Protocol violation: got another message after the response
                    return Err(ClientError::new(
                        Code::Unimplemented,
                        "unary response has multiple messages",
                    ));
                }
            }
        }

        // 11. Extract metadata from response headers
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(message, metadata))
    }

    /// Make a client-streaming RPC call with custom options.
    ///
    /// This is the same as [`call_client_stream`](Self::call_client_stream) but allows specifying
    /// per-call options like custom headers and timeout overrides.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    /// use futures::stream;
    ///
    /// let options = CallOptions::new()
    ///     .header("authorization", "Bearer token123");
    ///
    /// let messages = stream::iter(vec![
    ///     Message { content: "hello".to_string() },
    /// ]);
    ///
    /// let response = client.call_client_stream_with_options::<Message, Response, _>(
    ///     "chat.v1.ChatService/SendMessages",
    ///     messages,
    ///     options,
    /// ).await?;
    /// ```
    pub async fn call_client_stream_with_options<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
        options: CallOptions,
    ) -> Result<ConnectResponse<Res>, ClientError>
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "client_stream",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        // 3. Create streaming body
        let body = TransportBody::streaming(encoder);

        // 4. Build request with streaming content-type
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder =
                req_builder.header("connect-content-encoding", self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(t) = effective_timeout
            && let Some(timeout_ms) = duration_to_timeout_header(t)
        {
            req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
        }

        // Add custom headers from options (skip reserved protocol headers)
        for (name, value) in options.headers.iter() {
            if !is_reserved_header(name) {
                req_builder = req_builder.header(name, value);
            }
        }

        // Apply interceptors (header-only for streaming initial request)
        let mut interceptor_headers = http::HeaderMap::new();
        {
            let mut ctx = RequestContext::new(procedure, &mut interceptor_headers);
            // Use a unit placeholder - streaming interceptors use on_stream_send for messages
            self.interceptor.intercept_request(&mut ctx, &mut ())?;
        }
        for (name, value) in interceptor_headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Build request with body
        let req = req_builder
            .body(body)
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 5. Send request (with client-side timeout if configured)
        let response = if let Some(t) = effective_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??
        } else {
            self.transport.request(req).await?
        };

        // 6. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(
                status,
                &response_headers,
                body_bytes,
            ));
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = response_headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding =
            CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                ClientError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get the streaming body and decode the single response message
        let body = response.into_body();
        let byte_stream = body_to_stream(body);
        let mut decoder =
            FrameDecoder::<_, Res>::new(byte_stream, self.use_proto, response_encoding);

        // 9. Get the single response message
        let message = match decoder.next().await {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => return Err(e),
            None => {
                return Err(ClientError::Protocol(
                    "expected response message but stream ended".to_string(),
                ));
            }
        };

        // 10. Consume the EndStream frame and check for errors
        // The decoder will return an error if the EndStream frame contains an error
        if let Some(result) = decoder.next().await {
            match result {
                Err(e) => {
                    // EndStream contained an error - propagate it
                    return Err(e);
                }
                Ok(_) => {
                    // Protocol violation: got another message after the response
                    return Err(ClientError::new(
                        Code::Unimplemented,
                        "unary response has multiple messages",
                    ));
                }
            }
        }

        // 11. Extract metadata from response headers
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(message, metadata))
    }

    /// Make a bidirectional streaming RPC call.
    ///
    /// Both client and server send streams of messages. This requires HTTP/2
    /// for full duplex operation (both sides can send and receive simultaneously).
    ///
    /// # Arguments
    ///
    /// * `procedure` - The full procedure path (e.g., "my.package.MyService/BidiStream")
    /// * `request` - A stream of request messages
    ///
    /// # Returns
    ///
    /// Returns a [`ConnectResponse`] containing a [`Streaming`] that yields
    /// response messages. After the stream is consumed, trailers are available
    /// via `stream.trailers()`.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if:
    /// - The HTTP request fails
    /// - The server returns an error status immediately
    ///
    /// Individual stream items may also return errors if:
    /// - A message cannot be decoded
    /// - The server sends an error in the EndStream frame
    /// - The connection is lost
    ///
    /// # Note on HTTP/2
    ///
    /// Bidirectional streaming requires HTTP/2 for true full-duplex operation.
    /// Over HTTP/1.1, the request body must be fully sent before the response
    /// can be received, which defeats the purpose of bidirectional streaming.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use futures::{stream, StreamExt};
    ///
    /// let messages = stream::iter(vec![
    ///     EchoRequest { message: "hello".to_string() },
    ///     EchoRequest { message: "world".to_string() },
    /// ]);
    ///
    /// let response = client.call_bidi_stream::<EchoRequest, EchoResponse, _>(
    ///     "echo.v1.EchoService/EchoBidiStream",
    ///     messages,
    /// ).await?;
    ///
    /// let mut stream = response.into_inner();
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(msg) => println!("Got: {:?}", msg),
    ///         Err(e) => eprintln!("Error: {:?}", e),
    ///     }
    /// }
    ///
    /// // Access trailers after stream is consumed
    /// if let Some(trailers) = stream.trailers() {
    ///     println!("Trailers: {:?}", trailers);
    /// }
    /// ```
    pub async fn call_bidi_stream<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
    ) -> Result<
        ConnectResponse<
            Streaming<
                FrameDecoder<
                    impl futures::Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res, S>,
                    Res,
                >,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "bidi_stream",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        // 3. Create streaming body from encoder
        let body = TransportBody::streaming(encoder);

        // 4. Build request with streaming content-type
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder =
                req_builder.header("connect-content-encoding", self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(t) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Apply interceptors (header-only for streaming initial request)
        let mut interceptor_headers = http::HeaderMap::new();
        {
            let mut ctx = RequestContext::new(procedure, &mut interceptor_headers);
            // Use a unit placeholder - streaming interceptors use on_stream_send for messages
            self.interceptor.intercept_request(&mut ctx, &mut ())?;
        }
        for (name, value) in interceptor_headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Build request with body
        let req = req_builder
            .body(body)
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 5. Send request (with client-side timeout if configured)
        let response = if let Some(t) = self.default_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??
        } else {
            self.transport.request(req).await?
        };

        // 6. Verify HTTP/2 for bidirectional streaming
        // Bidi streaming requires HTTP/2 for full-duplex operation
        let version = response.version();
        if version < http::Version::HTTP_2 {
            return Err(ClientError::new(
                Code::Unimplemented,
                format!(
                    "bidirectional streaming requires HTTP/2, but server responded with {:?}",
                    version
                ),
            ));
        }

        // 7. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(
                status,
                &response_headers,
                body_bytes,
            ));
        }

        // 8. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = response_headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding =
            CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                ClientError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 9. Get the streaming body
        let body = response.into_body();
        let byte_stream = body_to_stream(body);

        // 10. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 11. Wrap with Streaming
        let stream_body = Streaming::new(decoder);

        // 12. Extract metadata from initial response headers
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(stream_body, metadata))
    }

    /// Make a bidirectional streaming RPC call with custom options.
    ///
    /// This is the same as [`call_bidi_stream`](Self::call_bidi_stream) but allows specifying
    /// per-call options like custom headers and timeout overrides.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::CallOptions;
    /// use futures::{stream, StreamExt};
    ///
    /// let options = CallOptions::new()
    ///     .header("authorization", "Bearer token123");
    ///
    /// let messages = stream::iter(vec![
    ///     EchoRequest { message: "hello".to_string() },
    /// ]);
    ///
    /// let response = client.call_bidi_stream_with_options::<EchoRequest, EchoResponse, _>(
    ///     "echo.v1.EchoService/EchoBidiStream",
    ///     messages,
    ///     options,
    /// ).await?;
    /// ```
    pub async fn call_bidi_stream_with_options<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
        options: CallOptions,
    ) -> Result<
        ConnectResponse<
            Streaming<
                FrameDecoder<
                    impl futures::Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res, S>,
                    Res,
                >,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        #[cfg(feature = "tracing")]
        let _span = info_span!(
            "rpc.call",
            rpc.method = %procedure,
            rpc.type = "bidi_stream",
            rpc.encoding = %self.encoding_name(),
            otel.kind = "client",
        )
        .entered();

        // 1. Build URL (strip leading slash from procedure to avoid double slashes)
        let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression.clone(),
        );

        // 3. Create streaming body
        let body = TransportBody::streaming(encoder);

        // 4. Build request with streaming content-type
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder =
                req_builder.header("connect-content-encoding", self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(t) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(t) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Add custom headers from options (skip reserved protocol headers)
        for (name, value) in options.headers.iter() {
            if !is_reserved_header(name) {
                req_builder = req_builder.header(name, value);
            }
        }

        // Apply interceptors (header-only for streaming initial request)
        let mut interceptor_headers = http::HeaderMap::new();
        {
            let mut ctx = RequestContext::new(procedure, &mut interceptor_headers);
            // Use a unit placeholder - streaming interceptors use on_stream_send for messages
            self.interceptor.intercept_request(&mut ctx, &mut ())?;
        }
        for (name, value) in interceptor_headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Build request with body
        let req = req_builder
            .body(body)
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;

        // 5. Send request (with client-side timeout if configured)
        let response = if let Some(t) = effective_timeout {
            timeout(t, self.transport.request(req))
                .await
                .map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??
        } else {
            self.transport.request(req).await?
        };

        // 6. Verify HTTP/2 for bidirectional streaming
        // Bidi streaming requires HTTP/2 for full-duplex operation
        let version = response.version();
        if version < http::Version::HTTP_2 {
            return Err(ClientError::new(
                Code::Unimplemented,
                format!(
                    "bidirectional streaming requires HTTP/2, but server responded with {:?}",
                    version
                ),
            ));
        }

        // 7. Check response status
        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Transport(format!("failed to read error body: {}", e)))?
                .to_bytes();
            return Err(decompress_and_parse_error(
                status,
                &response_headers,
                body_bytes,
            ));
        }

        // 8. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = response_headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding =
            CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                ClientError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 9. Get the streaming body
        let body = response.into_body();
        let byte_stream = body_to_stream(body);

        // 10. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 11. Wrap with Streaming
        let stream_body = Streaming::new(decoder);

        // 12. Extract metadata from initial response headers
        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(stream_body, metadata))
    }
}

/// Helper to decompress and parse error response body.
///
/// This handles the case where error responses may be compressed.
/// Per connect-go reference implementation, error responses can have Content-Encoding
/// and should be decompressed before parsing.
///
/// Follows connect-go behavior:
/// - If Content-Encoding is set but unknown, return CodeInternal error
/// - If Content-Encoding is not set, use raw bytes
/// - If decompression fails, fall back to creating error from HTTP status
fn decompress_and_parse_error(
    status: http::StatusCode,
    headers: &http::HeaderMap,
    body_bytes: Bytes,
) -> ClientError {
    // Check Content-Encoding header for potential compression
    let content_encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());

    // If no Content-Encoding header, parse raw bytes
    let Some(encoding_str) = content_encoding else {
        return parse_error_response(status, &body_bytes);
    };

    // Empty or identity encoding means no compression
    if encoding_str.is_empty() || encoding_str == "identity" {
        return parse_error_response(status, &body_bytes);
    }

    // Try to get the compression encoding
    let Some(encoding) = CompressionEncoding::from_header(Some(encoding_str)) else {
        // Unknown encoding - per connect-go, return CodeInternal error
        return ClientError::new(
            Code::Internal,
            format!("unknown encoding {:?} in error response", encoding_str),
        );
    };

    // Try to get the codec for this encoding
    let Some(codec) = encoding.codec() else {
        // Encoding known but codec not available (feature not enabled)
        return ClientError::new(
            Code::Internal,
            format!(
                "compression {:?} not available (feature not enabled)",
                encoding_str
            ),
        );
    };

    // Decompress and parse
    match codec.decompress(&body_bytes) {
        Ok(decompressed) => parse_error_response(status, &decompressed),
        Err(_) => {
            // Decompression failed - fall back to error from HTTP status
            // (consistent with connect-go behavior when unmarshaling fails)
            ClientError::new(
                crate::response::error_parser::http_status_to_code(status),
                format!("HTTP {}: decompression of error body failed", status),
            )
        }
    }
}

/// Convert a hyper Incoming body to a stream of bytes with ClientError.
fn body_to_stream(
    body: hyper::body::Incoming,
) -> impl futures::Stream<Item = Result<Bytes, ClientError>> + Unpin {
    use http_body_util::BodyExt;

    Box::pin(
        futures::stream::unfold(body, |mut body| async move {
            match body.frame().await {
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        Some((Ok(data), body))
                    } else {
                        // Trailers or other frame types - skip
                        Some((Ok(Bytes::new()), body))
                    }
                }
                Some(Err(e)) => Some((
                    Err(ClientError::Transport(format!("stream error: {}", e))),
                    body,
                )),
                None => None,
            }
        })
        .filter(|result| {
            // Filter out empty chunks
            futures::future::ready(match result {
                Ok(bytes) => !bytes.is_empty(),
                Err(_) => true,
            })
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unary_content_type_json() {
        let client = ConnectClient::builder("http://localhost:3000")
            .use_json()
            .build()
            .unwrap();
        assert_eq!(client.unary_content_type(), "application/json");
    }

    #[test]
    fn test_unary_content_type_proto() {
        let client = ConnectClient::builder("http://localhost:3000")
            .use_proto()
            .build()
            .unwrap();
        assert_eq!(client.unary_content_type(), "application/proto");
    }

    #[test]
    fn test_streaming_content_type_json() {
        let client = ConnectClient::builder("http://localhost:3000")
            .use_json()
            .build()
            .unwrap();
        assert_eq!(client.streaming_content_type(), "application/connect+json");
    }

    #[test]
    fn test_streaming_content_type_proto() {
        let client = ConnectClient::builder("http://localhost:3000")
            .use_proto()
            .build()
            .unwrap();
        assert_eq!(client.streaming_content_type(), "application/connect+proto");
    }
}
