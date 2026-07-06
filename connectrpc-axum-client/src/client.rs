//! Connect RPC client implementation.
//!
//! This module provides the main [`ConnectClient`] type for making RPC calls.

use bytes::Bytes;
use http::{Method, Request, header};
use http_body_util::BodyExt;
use tokio::time::timeout;

use connectrpc_axum_core::{
    Code, CompressionConfig, CompressionEncoding, DecompressError, wrap_envelope,
};
#[cfg(feature = "tracing")]
use tracing::{Instrument, info_span};

use crate::ClientError;
use crate::config::{
    CallOptions, InterceptorInternal, RequestContext, ResponseContext, StreamType,
    duration_to_timeout_header,
};
use crate::transport::{HyperTransport, TransportBody};
use futures::{Stream, StreamExt};
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use std::time::Duration;

use crate::builder::ClientBuilder;
use crate::request::FrameEncoder;
use crate::response::error_parser::parse_error_response;
use crate::response::{
    ConnectResponse, FrameDecoder, InterceptingSendStream, InterceptingStreaming, Metadata,
    SendInterceptorError, Streaming, take_send_interceptor_error,
};

/// A raw streaming RPC response: the decoded response stream (a [`FrameDecoder`]
/// over the response body, wrapped in [`Streaming`]) plus response metadata.
///
/// `B` is the byte stream backing the decoder and `Res` the response message type.
pub type RawStreamingResponse<B, Res> = ConnectResponse<Streaming<FrameDecoder<B, Res>>>;

#[cfg(feature = "tracing")]
macro_rules! rpc_call_span {
    ($client:expr, $procedure:expr, $rpc_type:literal, $body:block) => {{
        let span = info_span!(
            "rpc.call",
            rpc.method = %$procedure,
            rpc.type = $rpc_type,
            rpc.encoding = %$client.encoding_name(),
            otel.kind = "client",
        );
        (async move $body).instrument(span).await
    }};
}

#[cfg(not(feature = "tracing"))]
macro_rules! rpc_call_span {
    ($client:expr, $procedure:expr, $rpc_type:literal, $body:block) => {{ $body }};
}

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
    /// Maximum size in bytes of a received (decompressed) message.
    ///
    /// `None` means unlimited. Mirrors connect-go's `WithReadMaxBytes`.
    receive_max_bytes: Option<usize>,
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
        receive_max_bytes: Option<usize>,
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
            receive_max_bytes,
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

    /// Build the full request header map for a streaming call.
    ///
    /// Includes the streaming content type, protocol version, compression
    /// headers, the `Connect-Timeout-Ms` header, and (reserved-filtered)
    /// custom headers from `options`. Interceptors run against this map so
    /// they observe the complete set of request headers, mirroring the
    /// unary path.
    ///
    /// `announce_content_encoding` adds `Connect-Content-Encoding` up front
    /// for calls whose streaming body may contain compressed frames; calls
    /// that compress a single buffered message add the header themselves
    /// once they know whether compression was applied.
    fn streaming_request_headers(
        &self,
        options: &CallOptions,
        effective_timeout: Option<Duration>,
        announce_content_encoding: bool,
    ) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static(self.streaming_content_type()),
        );
        headers.insert(
            CONNECT_PROTOCOL_VERSION_HEADER,
            http::HeaderValue::from_static(CONNECT_PROTOCOL_VERSION),
        );

        // Announce Connect-Content-Encoding when the streaming body may
        // contain compressed frames
        if announce_content_encoding
            && !self.request_encoding.is_identity()
            && !self.compression.is_disabled()
        {
            headers.insert(
                "connect-content-encoding",
                http::HeaderValue::from_static(self.request_encoding.as_str()),
            );
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            headers.insert(
                "connect-accept-encoding",
                http::HeaderValue::from_static(accept.as_str()),
            );
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        if let Some(t) = effective_timeout
            && let Some(timeout_ms) = duration_to_timeout_header(t)
        {
            headers.insert(CONNECT_TIMEOUT_HEADER, timeout_ms.parse().unwrap());
        }

        // Add custom headers from options (skip reserved protocol headers).
        // Insert the first value for each name (overriding any default) and
        // append the rest so multi-valued headers are preserved.
        for name in options.headers.keys() {
            if is_reserved_header(name) {
                continue;
            }
            let mut values = options.headers.get_all(name).iter();
            if let Some(first) = values.next() {
                headers.insert(name.clone(), first.clone());
            }
            for value in values {
                headers.append(name.clone(), value.clone());
            }
        }

        headers
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
        self.call_unary_with_options(procedure, request, CallOptions::default())
            .await
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
        rpc_call_span!(self, procedure, "unary", {
            // Build headers (before RPC interceptor so it can modify them)
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static(self.unary_content_type()),
            );
            headers.insert(
                CONNECT_PROTOCOL_VERSION_HEADER,
                http::HeaderValue::from_static(CONNECT_PROTOCOL_VERSION),
            );

            // Add Connect-Timeout-Ms header (options timeout overrides default)
            let effective_timeout = options.timeout.or(self.default_timeout);
            if let Some(t) = effective_timeout
                && let Some(timeout_ms) = duration_to_timeout_header(t)
            {
                headers.insert(CONNECT_TIMEOUT_HEADER, timeout_ms.parse().unwrap());
            }

            // Add Accept-Encoding if configured
            if let Some(accept) = self.accept_encoding {
                headers.insert(
                    header::ACCEPT_ENCODING,
                    http::HeaderValue::from_static(accept.as_str()),
                );
            }

            // Add custom headers from options (skip reserved protocol headers).
            // Insert the first value for each name (overriding any default) and
            // append the rest so multi-valued headers are preserved.
            for name in options.headers.keys() {
                if is_reserved_header(name) {
                    continue;
                }
                let mut values = options.headers.get_all(name).iter();
                if let Some(first) = values.next() {
                    headers.insert(name.clone(), first.clone());
                }
                for value in values {
                    headers.append(name.clone(), value.clone());
                }
            }

            // Apply interceptors to the headers and the request message
            let mut request = request.clone();
            {
                let mut ctx = RequestContext::new(procedure, &mut headers);
                self.interceptor.intercept_request(&mut ctx, &mut request)?;
            }

            // Encode the request body and maybe compress it
            let body = self.encode_message(&request)?;
            let (body, compressed) = self.maybe_compress(body)?;

            // Add Content-Encoding if compressed
            if compressed {
                headers.insert(
                    header::CONTENT_ENCODING,
                    self.request_encoding.as_str().parse().unwrap(),
                );
            }

            // Build URL (strip leading slash from procedure to avoid double slashes)
            let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
            let url = format!("{}/{}", self.base_url, procedure);

            // Build the HTTP request with the final header map
            let mut req = Request::builder()
                .method(Method::POST)
                .uri(&url)
                .body(TransportBody::full(body))
                .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;
            *req.headers_mut() = headers;

            // Send the request and read the response body. The client-side
            // timeout covers the entire call: connection, request, and
            // response body. Reading stops early if the body exceeds the
            // receive limit so an oversized response cannot exhaust memory.
            let call = async {
                let response = self.transport.request(req).await?;

                let status = response.status();
                let response_headers = response.headers().clone();

                let body = collect_body_limited(
                    response.into_body(),
                    self.receive_max_bytes,
                    "response body",
                )
                .await?;

                Ok::<_, ClientError>((status, response_headers, body))
            };
            let (status, response_headers, body) = match effective_timeout {
                Some(t) => timeout(t, call).await.map_err(|_| {
                    ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                })??,
                None => call.await?,
            };

            // Check response status
            if !status.is_success() {
                return Err(match body {
                    LimitedBody::Complete(body_bytes) => decompress_and_parse_error(
                        status,
                        &response_headers,
                        body_bytes,
                        self.receive_max_bytes,
                    ),
                    LimitedBody::TooLarge => error_body_exceeds_limit(status),
                });
            }

            let body_bytes = match body {
                LimitedBody::Complete(body_bytes) => body_bytes,
                LimitedBody::TooLarge => {
                    let limit = self.receive_max_bytes.unwrap_or(usize::MAX);
                    return Err(ClientError::new(
                        Code::ResourceExhausted,
                        format!("message size exceeds maximum allowed size of {limit} bytes"),
                    ));
                }
            };

            // Handle response decompression
            let content_encoding = response_headers
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok());

            let response_encoding =
                CompressionEncoding::from_header(content_encoding).ok_or_else(|| {
                    ClientError::Protocol(format!(
                        "unsupported response encoding: {:?}",
                        content_encoding
                    ))
                })?;

            // Decompress bounded by the receive limit, so a decompression
            // bomb cannot exhaust memory
            let body_bytes = if let Some(codec) = response_encoding.codec() {
                codec
                    .decompress_limited(&body_bytes, self.receive_max_bytes.unwrap_or(usize::MAX))
                    .map_err(|e| match e {
                        DecompressError::TooLarge { limit } => ClientError::new(
                            Code::ResourceExhausted,
                            format!("message size exceeds maximum allowed size of {limit} bytes"),
                        ),
                        e => ClientError::Decode(format!("decompression failed: {}", e)),
                    })?
            } else {
                body_bytes
            };

            // Decode the response
            let mut message: Res = self.decode_message(&body_bytes)?;

            // Apply interceptors to the response
            {
                let ctx = ResponseContext::new(procedure, &response_headers);
                self.interceptor.intercept_response(&ctx, &mut message)?;
            }

            // Extract metadata
            let metadata = Metadata::new(response_headers);

            Ok(ConnectResponse::new(message, metadata))
        })
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
            InterceptingStreaming<
                FrameDecoder<
                    impl futures::Stream<Item = Result<Bytes, ClientError>>
                    + Unpin
                    + use<'_, I, Req, Res>,
                    Res,
                >,
                Res,
                I,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + Clone + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
    {
        self.call_server_stream_with_options(procedure, request, CallOptions::default())
            .await
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
            InterceptingStreaming<
                FrameDecoder<
                    impl Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res>,
                    Res,
                >,
                Res,
                I,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + Clone + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
    {
        rpc_call_span!(self, procedure, "server_stream", {
            // Build headers (before interceptors so they can modify them)
            let effective_timeout = options.timeout.or(self.default_timeout);
            let mut headers = self.streaming_request_headers(&options, effective_timeout, false);

            // Apply interceptors to the headers and the request message.
            // The single request of a server-streaming call goes through
            // `intercept_request`, mirroring the unary path.
            let mut request = request.clone();
            {
                let mut ctx = RequestContext::new(procedure, &mut headers);
                self.interceptor.intercept_request(&mut ctx, &mut request)?;
            }

            // Encode the request body and maybe compress it
            let body = self.encode_message(&request)?;
            let (body, compressed) = self.maybe_compress(body)?;

            // Wrap in envelope for streaming request
            // Connect streaming protocol requires envelope framing even for single-message requests
            let body = Bytes::from(wrap_envelope(&body, compressed)?);

            // Add Connect-Content-Encoding if compressed (streaming uses this header, not Content-Encoding)
            if compressed {
                headers.insert(
                    "connect-content-encoding",
                    self.request_encoding.as_str().parse().unwrap(),
                );
            }

            // Build URL (strip leading slash from procedure to avoid double slashes)
            let procedure = procedure.strip_prefix('/').unwrap_or(procedure);
            let url = format!("{}/{}", self.base_url, procedure);

            // Build the HTTP request with the final header map
            let mut req = Request::builder()
                .method(Method::POST)
                .uri(&url)
                .body(TransportBody::full(body))
                .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;
            let request_headers = headers.clone();
            *req.headers_mut() = headers;

            // Send request (with client-side timeout if configured; for
            // server-streaming the timeout applies until the response
            // headers arrive, not to stream consumption)
            let response = if let Some(t) = effective_timeout {
                timeout(t, self.transport.request(req))
                    .await
                    .map_err(|_| {
                        ClientError::new(Code::DeadlineExceeded, "client timeout exceeded")
                    })??
            } else {
                self.transport.request(req).await?
            };

            // Check response status
            let status = response.status();
            let response_headers = response.headers().clone();

            if !status.is_success() {
                let body = collect_body_limited(
                    response.into_body(),
                    self.receive_max_bytes,
                    "error body",
                )
                .await?;
                return Err(match body {
                    LimitedBody::Complete(body_bytes) => decompress_and_parse_error(
                        status,
                        &response_headers,
                        body_bytes,
                        self.receive_max_bytes,
                    ),
                    LimitedBody::TooLarge => error_body_exceeds_limit(status),
                });
            }

            // Get compression encoding from Connect-Content-Encoding header
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

            // Get the streaming body
            let body = response.into_body();
            let byte_stream = body_to_stream(body);

            // Wrap with FrameDecoder
            let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding)
                .with_max_message_size(self.receive_max_bytes);

            // Wrap with Streaming
            let stream_body = Streaming::new(decoder);

            // Wrap with InterceptingStreaming for per-message interception
            let intercepting_stream = InterceptingStreaming::new(
                stream_body,
                self.interceptor.clone(),
                procedure.to_string(),
                StreamType::ServerStream,
                request_headers,
                response_headers.clone(),
            );

            // Extract metadata from initial response headers
            let metadata = Metadata::new(response_headers);

            Ok(ConnectResponse::new(intercepting_stream, metadata))
        })
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
    /// - An `on_stream_send` interceptor fails. This aborts the HTTP request
    ///   body mid-stream (the server sees a broken request, not a clean
    ///   end-of-stream) and the interceptor error is returned, taking
    ///   precedence over any transport or server error.
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
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        self.call_client_stream_with_options(procedure, request, CallOptions::default())
            .await
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
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        rpc_call_span!(self, procedure, "client_stream", {
            // Strip leading slash from procedure to avoid double slashes in the URL
            let procedure = procedure.strip_prefix('/').unwrap_or(procedure);

            // Build the full request header map, then let interceptors mutate
            // it (message interception for streaming requests happens per
            // message via on_stream_send, so the request placeholder is unit)
            let effective_timeout = options.timeout.or(self.default_timeout);
            let mut headers = self.streaming_request_headers(&options, effective_timeout, true);
            {
                let mut ctx = RequestContext::new(procedure, &mut headers);
                self.interceptor.intercept_request(&mut ctx, &mut ())?;
            }

            // Wrap request stream with InterceptingSendStream for per-message interception
            // Client-streaming awaits the unary response, so captured send errors are checked before return.
            let send_error = SendInterceptorError::new();
            let intercepting_stream = InterceptingSendStream::with_send_error_capture(
                request,
                self.interceptor.clone(),
                procedure.to_string(),
                StreamType::ClientStream,
                headers.clone(),
                send_error.clone(),
            );

            self.call_client_stream_fallible_with_headers(
                procedure,
                intercepting_stream,
                options,
                headers,
                send_error,
            )
            .await
        })
    }

    /// Make a client-streaming RPC call from a fallible request stream.
    ///
    /// Used by generated clients whose typed `on_send` interceptors can fail.
    /// An `Err` item from `request` aborts the HTTP request body mid-stream
    /// (the server sees a broken request, not a clean end-of-stream). The error
    /// recorded in `send_error` takes precedence over any transport or server
    /// error on every return path, including a successful response.
    #[doc(hidden)]
    pub async fn call_client_stream_fallible_with_options<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
        options: CallOptions,
        send_error: Arc<SendInterceptorError>,
    ) -> Result<ConnectResponse<Res>, ClientError>
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Result<Req, ClientError>> + Send + Unpin + 'static,
    {
        rpc_call_span!(self, procedure, "client_stream", {
            let procedure = procedure.strip_prefix('/').unwrap_or(procedure);

            let effective_timeout = options.timeout.or(self.default_timeout);
            let mut headers = self.streaming_request_headers(&options, effective_timeout, true);
            {
                let mut ctx = RequestContext::new(procedure, &mut headers);
                self.interceptor.intercept_request(&mut ctx, &mut ())?;
            }

            self.call_client_stream_fallible_with_headers(
                procedure, request, options, headers, send_error,
            )
            .await
        })
    }

    async fn call_client_stream_fallible_with_headers<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
        options: CallOptions,
        headers: http::HeaderMap,
        send_error: Arc<SendInterceptorError>,
    ) -> Result<ConnectResponse<Res>, ClientError>
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Result<Req, ClientError>> + Send + Unpin + 'static,
    {
        let url = format!("{}/{}", self.base_url, procedure);

        let encoder: FrameEncoder<_, Req> = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        let body = TransportBody::streaming(encoder);

        // Build the HTTP request with the final header map
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .body(body)
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;
        *req.headers_mut() = headers;

        // Send the request and read the single response message. Like the
        // unary path, the client-side timeout covers the entire call:
        // request, response headers, and reading the response body.
        let effective_timeout = options.timeout.or(self.default_timeout);
        let call = async {
            let response = match self.transport.request(req).await {
                Ok(response) => response,
                Err(e) => {
                    if let Some(send_error) = take_send_interceptor_error(&send_error) {
                        return Err(send_error);
                    }
                    return Err(e);
                }
            };

            if let Some(send_error) = take_send_interceptor_error(&send_error) {
                return Err(send_error);
            }

            let status = response.status();
            let response_headers = response.headers().clone();

            if !status.is_success() {
                let body_result = collect_body_limited(
                    response.into_body(),
                    self.receive_max_bytes,
                    "error body",
                )
                .await;
                // The transport may still be polling the request body while the
                // error body is collected, so a send interceptor error can be
                // recorded after the check above.
                if let Some(send_error) = take_send_interceptor_error(&send_error) {
                    return Err(send_error);
                }
                return Err(match body_result? {
                    LimitedBody::Complete(body_bytes) => decompress_and_parse_error(
                        status,
                        &response_headers,
                        body_bytes,
                        self.receive_max_bytes,
                    ),
                    LimitedBody::TooLarge => error_body_exceeds_limit(status),
                });
            }

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

            let body = response.into_body();
            let byte_stream = body_to_stream(body);
            let mut decoder =
                FrameDecoder::<_, Res>::new(byte_stream, self.use_proto, response_encoding)
                    .with_max_message_size(self.receive_max_bytes);

            let message = match decoder.next().await {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => {
                    if let Some(send_error) = take_send_interceptor_error(&send_error) {
                        return Err(send_error);
                    }
                    return Err(e);
                }
                None => {
                    if let Some(send_error) = take_send_interceptor_error(&send_error) {
                        return Err(send_error);
                    }
                    return Err(ClientError::Protocol(
                        "expected response message but stream ended".to_string(),
                    ));
                }
            };

            // The decoder will return an error if the EndStream frame contains an error
            if let Some(result) = decoder.next().await {
                match result {
                    Err(e) => {
                        if let Some(send_error) = take_send_interceptor_error(&send_error) {
                            return Err(send_error);
                        }
                        // EndStream contained an error - propagate it
                        return Err(e);
                    }
                    Ok(_) => {
                        if let Some(send_error) = take_send_interceptor_error(&send_error) {
                            return Err(send_error);
                        }
                        // Protocol violation: got another message after the response
                        return Err(ClientError::new(
                            Code::Unimplemented,
                            "unary response has multiple messages",
                        ));
                    }
                }
            }

            if let Some(send_error) = take_send_interceptor_error(&send_error) {
                return Err(send_error);
            }

            Ok::<_, ClientError>((message, response_headers))
        };
        let (mut message, response_headers) = match effective_timeout {
            Some(t) => match timeout(t, call).await {
                Ok(result) => result?,
                Err(_) => {
                    // A send interceptor error still takes precedence over
                    // the client-side timeout.
                    if let Some(send_error) = take_send_interceptor_error(&send_error) {
                        return Err(send_error);
                    }
                    return Err(ClientError::new(
                        Code::DeadlineExceeded,
                        "client timeout exceeded",
                    ));
                }
            },
            None => call.await?,
        };

        // Apply interceptors to the single response of the client-streaming
        // call, mirroring the unary path.
        {
            let ctx = ResponseContext::new(procedure, &response_headers);
            self.interceptor.intercept_response(&ctx, &mut message)?;
        }

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
    /// - A send interceptor fails, which takes precedence over buffered received messages
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
            InterceptingStreaming<
                FrameDecoder<
                    impl futures::Stream<Item = Result<Bytes, ClientError>>
                    + Unpin
                    + use<'_, I, Req, Res, S>,
                    Res,
                >,
                Res,
                I,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        self.call_bidi_stream_with_options(procedure, request, CallOptions::default())
            .await
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
            InterceptingStreaming<
                FrameDecoder<
                    impl futures::Stream<Item = Result<Bytes, ClientError>>
                    + Unpin
                    + use<'_, I, Req, Res, S>,
                    Res,
                >,
                Res,
                I,
            >,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Req> + Send + Unpin + 'static,
    {
        rpc_call_span!(self, procedure, "bidi_stream", {
            // Strip leading slash from procedure to avoid double slashes in the URL
            let procedure = procedure.strip_prefix('/').unwrap_or(procedure);

            // Build the full request header map, then let interceptors mutate
            // it (message interception for streaming requests happens per
            // message via on_stream_send, so the request placeholder is unit)
            let effective_timeout = options.timeout.or(self.default_timeout);
            let mut headers = self.streaming_request_headers(&options, effective_timeout, true);
            {
                let mut ctx = RequestContext::new(procedure, &mut headers);
                self.interceptor.intercept_request(&mut ctx, &mut ())?;
            }

            // Wrap request stream with InterceptingSendStream for per-message interception
            // Bidi returns a receive stream, so captured send errors also wake pending receive polls.
            let send_error = SendInterceptorError::new();
            let intercepting_stream = InterceptingSendStream::with_send_error_capture(
                request,
                self.interceptor.clone(),
                procedure.to_string(),
                StreamType::BidiStream,
                headers.clone(),
                send_error.clone(),
            );

            let response = self
                .call_bidi_stream_fallible_with_headers(
                    procedure,
                    intercepting_stream,
                    options,
                    headers.clone(),
                    send_error.clone(),
                )
                .await?;
            let response_headers = response.metadata().headers().clone();

            Ok(response.map(|stream_body| {
                InterceptingStreaming::with_send_error_capture(
                    stream_body,
                    self.interceptor.clone(),
                    procedure.to_string(),
                    StreamType::BidiStream,
                    headers,
                    response_headers,
                    send_error,
                )
            }))
        })
    }

    /// Make a bidirectional streaming RPC call from a fallible request stream.
    ///
    /// Used by generated clients whose typed `on_send` interceptors can fail.
    /// An `Err` item from `request` aborts the HTTP request body mid-stream
    /// (the server sees a broken request, not a clean end-of-stream). The error
    /// recorded in `send_error` takes precedence over transport and call-start
    /// errors; callers wrapping the returned stream should pass the same
    /// `send_error` to a receive wrapper so it also wins over receive errors.
    #[doc(hidden)]
    pub async fn call_bidi_stream_fallible_with_options<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
        options: CallOptions,
        send_error: Arc<SendInterceptorError>,
    ) -> Result<
        RawStreamingResponse<
            impl futures::Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res, S>,
            Res,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Result<Req, ClientError>> + Send + Unpin + 'static,
    {
        rpc_call_span!(self, procedure, "bidi_stream", {
            let procedure = procedure.strip_prefix('/').unwrap_or(procedure);

            let effective_timeout = options.timeout.or(self.default_timeout);
            let mut headers = self.streaming_request_headers(&options, effective_timeout, true);
            {
                let mut ctx = RequestContext::new(procedure, &mut headers);
                self.interceptor.intercept_request(&mut ctx, &mut ())?;
            }

            self.call_bidi_stream_fallible_with_headers(
                procedure, request, options, headers, send_error,
            )
            .await
        })
    }

    async fn call_bidi_stream_fallible_with_headers<Req, Res, S>(
        &self,
        procedure: &str,
        request: S,
        options: CallOptions,
        headers: http::HeaderMap,
        send_error: Arc<SendInterceptorError>,
    ) -> Result<
        RawStreamingResponse<
            impl futures::Stream<Item = Result<Bytes, ClientError>> + Unpin + use<'_, I, Req, Res, S>,
            Res,
        >,
        ClientError,
    >
    where
        Req: Message + Serialize + 'static,
        Res: Message + DeserializeOwned + Default + 'static,
        S: Stream<Item = Result<Req, ClientError>> + Send + Unpin + 'static,
    {
        let url = format!("{}/{}", self.base_url, procedure);

        let encoder: FrameEncoder<_, Req> = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        let body = TransportBody::streaming(encoder);

        // Build the HTTP request with the final header map
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(&url)
            .body(body)
            .map_err(|e| ClientError::Protocol(format!("failed to build request: {}", e)))?;
        *req.headers_mut() = headers;

        // Send request (with client-side timeout if configured; for bidi
        // streaming the timeout applies until the response headers arrive,
        // not to stream consumption)
        let effective_timeout = options.timeout.or(self.default_timeout);
        let response_result = if let Some(t) = effective_timeout {
            match timeout(t, self.transport.request(req)).await {
                Ok(result) => result,
                Err(_) => Err(ClientError::new(
                    Code::DeadlineExceeded,
                    "client timeout exceeded",
                )),
            }
        } else {
            self.transport.request(req).await
        };
        let response = match response_result {
            Ok(response) => response,
            Err(e) => {
                if let Some(send_error) = take_send_interceptor_error(&send_error) {
                    return Err(send_error);
                }
                return Err(e);
            }
        };

        if let Some(send_error) = take_send_interceptor_error(&send_error) {
            return Err(send_error);
        }

        let status = response.status();
        let response_headers = response.headers().clone();

        if !status.is_success() {
            let body_result =
                collect_body_limited(response.into_body(), self.receive_max_bytes, "error body")
                    .await;
            // The transport may still be polling the request body while the
            // error body is collected, so a send interceptor error can be
            // recorded after the check above.
            if let Some(send_error) = take_send_interceptor_error(&send_error) {
                return Err(send_error);
            }
            return Err(match body_result? {
                LimitedBody::Complete(body_bytes) => decompress_and_parse_error(
                    status,
                    &response_headers,
                    body_bytes,
                    self.receive_max_bytes,
                ),
                LimitedBody::TooLarge => error_body_exceeds_limit(status),
            });
        }

        // Bidi streaming requires HTTP/2 for full-duplex operation. Checked
        // after the HTTP status so a server error is reported as itself
        // rather than masked by the version mismatch.
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

        let body = response.into_body();
        let byte_stream = body_to_stream(body);

        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding)
            .with_max_message_size(self.receive_max_bytes);

        let stream_body = Streaming::new(decoder);

        let metadata = Metadata::new(response_headers);

        Ok(ConnectResponse::new(stream_body, metadata))
    }
}

/// Outcome of reading a response body under the client's receive limit.
enum LimitedBody {
    Complete(Bytes),
    /// The body exceeded `receive_max_bytes`; reading stopped early.
    TooLarge,
}

/// Collect a response body, stopping as soon as more than `limit` bytes have
/// been buffered so an oversized (or unbounded) body cannot exhaust memory.
/// `context` names the body in transport error messages.
async fn collect_body_limited(
    body: hyper::body::Incoming,
    limit: Option<usize>,
    context: &str,
) -> Result<LimitedBody, ClientError> {
    let limit = limit.unwrap_or(usize::MAX);
    let mut buf = bytes::BytesMut::new();
    let mut body = std::pin::pin!(body);
    while let Some(frame) = body.frame().await {
        let frame =
            frame.map_err(|e| ClientError::Transport(format!("failed to read {context}: {e}")))?;
        if let Ok(data) = frame.into_data() {
            if buf.len().saturating_add(data.len()) > limit {
                return Ok(LimitedBody::TooLarge);
            }
            buf.extend_from_slice(&data);
        }
    }
    Ok(LimitedBody::Complete(buf.freeze()))
}

/// Error for a non-2xx response whose body could not be processed within
/// `receive_max_bytes`: the error details are dropped, but the code derived
/// from the HTTP status is still delivered to the caller (graceful
/// degradation, mirroring the strategy adopted for oversized EndStream
/// frames on the server side).
fn error_body_exceeds_limit(status: http::StatusCode) -> ClientError {
    ClientError::new(
        crate::response::error_parser::http_status_to_code(status),
        format!("HTTP {status}: error body exceeds receive_max_bytes; error details omitted"),
    )
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
///
/// Decompression is bounded by `receive_max_bytes`; an error body expanding
/// past the limit degrades to an HTTP-status-derived error instead of
/// buffering unbounded output.
fn decompress_and_parse_error(
    status: http::StatusCode,
    headers: &http::HeaderMap,
    body_bytes: Bytes,
    receive_max_bytes: Option<usize>,
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

    // Decompress (bounded by the receive limit) and parse
    match codec.decompress_limited(&body_bytes, receive_max_bytes.unwrap_or(usize::MAX)) {
        Ok(decompressed) => parse_error_response(status, &decompressed),
        Err(DecompressError::TooLarge { .. }) => error_body_exceeds_limit(status),
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

    /// End-to-end tests against a local axum server exercising interceptor
    /// coverage, timeouts, and receive-size limits.
    mod e2e {
        use super::*;
        use crate::config::{Interceptor, MessageInterceptor, StreamContext};
        use axum::Router;
        use axum::routing::post;
        use std::sync::Mutex;

        #[derive(Clone, PartialEq, Default, Debug)]
        struct TestMessage {
            value: String,
        }

        impl serde::Serialize for TestMessage {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct("TestMessage", 1)?;
                state.serialize_field("value", &self.value)?;
                state.end()
            }
        }

        impl<'de> serde::Deserialize<'de> for TestMessage {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                #[derive(serde::Deserialize)]
                struct Helper {
                    value: String,
                }
                let helper = Helper::deserialize(deserializer)?;
                Ok(TestMessage {
                    value: helper.value,
                })
            }
        }

        impl Message for TestMessage {
            fn encode_raw(&self, buf: &mut impl bytes::BufMut)
            where
                Self: Sized,
            {
                if !self.value.is_empty() {
                    prost::encoding::string::encode(1, &self.value, buf);
                }
            }

            fn merge_field(
                &mut self,
                tag: u32,
                wire_type: prost::encoding::WireType,
                buf: &mut impl bytes::Buf,
                ctx: prost::encoding::DecodeContext,
            ) -> Result<(), prost::DecodeError>
            where
                Self: Sized,
            {
                if tag == 1 {
                    prost::encoding::string::merge(wire_type, &mut self.value, buf, ctx)
                } else {
                    prost::encoding::skip_field(wire_type, tag, buf, ctx)
                }
            }

            fn encoded_len(&self) -> usize {
                if self.value.is_empty() {
                    0
                } else {
                    prost::encoding::string::encoded_len(1, &self.value)
                }
            }

            fn clear(&mut self) {
                self.value.clear();
            }
        }

        /// Start an axum server on an ephemeral port; returns its base URL.
        async fn spawn_server(app: Router) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            format!("http://{}", addr)
        }

        /// Build a Connect streaming envelope frame.
        fn make_frame(flags: u8, payload: &[u8]) -> Vec<u8> {
            let mut frame = Vec::with_capacity(5 + payload.len());
            frame.push(flags);
            frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            frame.extend_from_slice(payload);
            frame
        }

        /// A Connect streaming response body: one JSON message frame plus an
        /// empty EndStream frame.
        fn streaming_response_body(message_json: &[u8]) -> Vec<u8> {
            let mut body = make_frame(0x00, message_json);
            body.extend_from_slice(&make_frame(0x02, b"{}"));
            body
        }

        fn streaming_response(message_json: &[u8]) -> axum::response::Response {
            http::Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, "application/connect+json")
                .header("x-server", "yes")
                .body(axum::body::Body::from(streaming_response_body(
                    message_json,
                )))
                .unwrap()
        }

        /// Captured request headers and body from the test server.
        type Captured = Arc<Mutex<Option<(http::HeaderMap, Bytes)>>>;

        fn capturing_route(path: &str, captured: Captured, message_json: &'static [u8]) -> Router {
            Router::new().route(
                path,
                post(move |headers: http::HeaderMap, body: Bytes| {
                    let captured = captured.clone();
                    async move {
                        *captured.lock().unwrap() = Some((headers, body));
                        streaming_response(message_json)
                    }
                }),
            )
        }

        /// Header-level interceptor that records the headers it observes and
        /// adds one of its own.
        #[derive(Clone)]
        struct RecordingHeaderInterceptor {
            seen: Arc<Mutex<Option<http::HeaderMap>>>,
        }

        impl Interceptor for RecordingHeaderInterceptor {
            fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
                *self.seen.lock().unwrap() = Some(ctx.headers.clone());
                ctx.headers.insert("x-intercepted", "1".parse().unwrap());
                Ok(())
            }
        }

        /// Message interceptor that rewrites request/response payloads and
        /// records the response headers it observes.
        #[derive(Clone)]
        struct MutatingMessageInterceptor {
            response_headers: Arc<Mutex<Option<http::HeaderMap>>>,
        }

        impl MessageInterceptor for MutatingMessageInterceptor {
            fn on_request<Req>(
                &self,
                _ctx: &mut RequestContext,
                request: &mut Req,
            ) -> Result<(), ClientError>
            where
                Req: Message + Serialize + 'static,
            {
                use std::any::Any;
                if let Some(msg) = (request as &mut dyn Any).downcast_mut::<TestMessage>() {
                    msg.value = format!("{}-req-intercepted", msg.value);
                }
                Ok(())
            }

            fn on_response<Res>(
                &self,
                ctx: &ResponseContext,
                response: &mut Res,
            ) -> Result<(), ClientError>
            where
                Res: Message + DeserializeOwned + Default + 'static,
            {
                use std::any::Any;
                *self.response_headers.lock().unwrap() = Some(ctx.headers.clone());
                if let Some(msg) = (response as &mut dyn Any).downcast_mut::<TestMessage>() {
                    msg.value = format!("{}-res-intercepted", msg.value);
                }
                Ok(())
            }

            fn on_stream_send<Req>(
                &self,
                _ctx: &StreamContext,
                _request: &mut Req,
            ) -> Result<(), ClientError>
            where
                Req: Message + Serialize + 'static,
            {
                Ok(())
            }
        }

        /// The single request of a server-streaming call goes through message
        /// interceptors, and header interceptors observe the complete header
        /// map (not an empty one) exactly once.
        #[tokio::test]
        async fn test_server_stream_request_interceptors_and_headers() {
            let captured: Captured = Arc::new(Mutex::new(None));
            let app = capturing_route(
                "/test.Service/ServerStream",
                captured.clone(),
                br#"{"value":"resp"}"#,
            );
            let base_url = spawn_server(app).await;

            let seen = Arc::new(Mutex::new(None));
            let client = ConnectClient::builder(&base_url)
                .with_interceptor(RecordingHeaderInterceptor { seen: seen.clone() })
                .with_message_interceptor(MutatingMessageInterceptor {
                    response_headers: Arc::new(Mutex::new(None)),
                })
                .build()
                .unwrap();

            let options = CallOptions::new().header("x-custom", "v");
            let response = client
                .call_server_stream_with_options::<TestMessage, TestMessage>(
                    "test.Service/ServerStream",
                    &TestMessage {
                        value: "hello".to_string(),
                    },
                    options,
                )
                .await
                .unwrap();

            let mut stream = response.into_inner();
            let msg = stream.next().await.unwrap().unwrap();
            assert_eq!(msg.value, "resp");
            assert!(stream.next().await.is_none());

            // The interceptor saw the full header map, not an empty one.
            let seen = seen.lock().unwrap().take().unwrap();
            assert_eq!(
                seen.get(header::CONTENT_TYPE).unwrap(),
                "application/connect+json"
            );
            assert_eq!(seen.get(CONNECT_PROTOCOL_VERSION_HEADER).unwrap(), "1");
            assert_eq!(seen.get("x-custom").unwrap(), "v");

            // The server received the message mutated by the request
            // interceptor, and each header exactly once (no duplicates).
            let (headers, body) = captured.lock().unwrap().take().unwrap();
            assert_eq!(headers.get_all("x-intercepted").iter().count(), 1);
            assert_eq!(headers.get_all("x-custom").iter().count(), 1);
            assert_eq!(headers.get_all(header::CONTENT_TYPE).iter().count(), 1);

            let (flags, length) = connectrpc_axum_core::parse_envelope_header(&body).unwrap();
            assert_eq!(flags, 0);
            let sent: serde_json::Value =
                serde_json::from_slice(&body[5..5 + length as usize]).unwrap();
            assert_eq!(sent["value"], "hello-req-intercepted");
        }

        /// The single response of a client-streaming call goes through
        /// response interceptors with the real response headers, and request
        /// interceptors observe the complete header map exactly once.
        #[tokio::test]
        async fn test_client_stream_response_interceptors_and_headers() {
            let captured: Captured = Arc::new(Mutex::new(None));
            let app = capturing_route(
                "/test.Service/ClientStream",
                captured.clone(),
                br#"{"value":"resp"}"#,
            );
            let base_url = spawn_server(app).await;

            let seen = Arc::new(Mutex::new(None));
            let response_headers = Arc::new(Mutex::new(None));
            let client = ConnectClient::builder(&base_url)
                .with_interceptor(RecordingHeaderInterceptor { seen: seen.clone() })
                .with_message_interceptor(MutatingMessageInterceptor {
                    response_headers: response_headers.clone(),
                })
                .build()
                .unwrap();

            let options = CallOptions::new().header("x-custom", "v");
            let messages = futures::stream::iter(vec![TestMessage {
                value: "one".to_string(),
            }]);
            let response = client
                .call_client_stream_with_options::<TestMessage, TestMessage, _>(
                    "test.Service/ClientStream",
                    messages,
                    options,
                )
                .await
                .unwrap();

            // The response interceptor ran on the decoded message with the
            // real response headers in its context.
            assert_eq!(response.into_inner().value, "resp-res-intercepted");
            let response_headers = response_headers.lock().unwrap().take().unwrap();
            assert_eq!(response_headers.get("x-server").unwrap(), "yes");

            // The request interceptor saw the full header map.
            let seen = seen.lock().unwrap().take().unwrap();
            assert_eq!(
                seen.get(header::CONTENT_TYPE).unwrap(),
                "application/connect+json"
            );
            assert_eq!(seen.get(CONNECT_PROTOCOL_VERSION_HEADER).unwrap(), "1");
            assert_eq!(seen.get("x-custom").unwrap(), "v");

            // The server received each header exactly once (no duplicates).
            let (headers, _body) = captured.lock().unwrap().take().unwrap();
            assert_eq!(headers.get_all("x-intercepted").iter().count(), 1);
            assert_eq!(headers.get_all("x-custom").iter().count(), 1);
            assert_eq!(headers.get_all(header::CONTENT_TYPE).iter().count(), 1);
        }

        fn stalled_body_response(content_type: &'static str) -> axum::response::Response {
            http::Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, content_type)
                .body(axum::body::Body::from_stream(futures::stream::pending::<
                    Result<Bytes, std::io::Error>,
                >()))
                .unwrap()
        }

        /// The unary timeout covers reading the response body, not just the
        /// arrival of the response headers.
        #[tokio::test]
        async fn test_unary_timeout_covers_response_body() {
            let app = Router::new().route(
                "/test.Service/Unary",
                post(|| async { stalled_body_response("application/json") }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .timeout(Duration::from_millis(200))
                .build()
                .unwrap();

            let err = client
                .call_unary::<TestMessage, TestMessage>(
                    "test.Service/Unary",
                    &TestMessage::default(),
                )
                .await
                .unwrap_err();
            assert_eq!(err.code(), Code::DeadlineExceeded);
        }

        /// The client-streaming timeout covers reading the single response
        /// message, not just the arrival of the response headers.
        #[tokio::test]
        async fn test_client_stream_timeout_covers_response_body() {
            let app = Router::new().route(
                "/test.Service/ClientStream",
                post(|| async { stalled_body_response("application/connect+json") }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .timeout(Duration::from_millis(200))
                .build()
                .unwrap();

            let messages = futures::stream::iter(vec![TestMessage::default()]);
            let err = client
                .call_client_stream::<TestMessage, TestMessage, _>(
                    "test.Service/ClientStream",
                    messages,
                )
                .await
                .unwrap_err();
            assert_eq!(err.code(), Code::DeadlineExceeded);
        }

        /// `receive_max_bytes` rejects an oversized unary response body.
        #[tokio::test]
        async fn test_unary_receive_max_bytes() {
            let app = Router::new().route(
                "/test.Service/Unary",
                post(|| async {
                    let body = format!(r#"{{"value":"{}"}}"#, "a".repeat(64 * 1024));
                    http::Response::builder()
                        .status(200)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap()
                }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .receive_max_bytes(1024)
                .build()
                .unwrap();

            let err = client
                .call_unary::<TestMessage, TestMessage>(
                    "test.Service/Unary",
                    &TestMessage::default(),
                )
                .await
                .unwrap_err();
            assert_eq!(err.code(), Code::ResourceExhausted);
        }

        /// `receive_max_bytes` rejects an oversized server-streaming message.
        #[tokio::test]
        async fn test_server_stream_receive_max_bytes() {
            let app = Router::new().route(
                "/test.Service/ServerStream",
                post(|| async {
                    let message = format!(r#"{{"value":"{}"}}"#, "a".repeat(64 * 1024));
                    http::Response::builder()
                        .status(200)
                        .header(header::CONTENT_TYPE, "application/connect+json")
                        .body(axum::body::Body::from(streaming_response_body(
                            message.as_bytes(),
                        )))
                        .unwrap()
                }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .receive_max_bytes(1024)
                .build()
                .unwrap();

            let response = client
                .call_server_stream::<TestMessage, TestMessage>(
                    "test.Service/ServerStream",
                    &TestMessage::default(),
                )
                .await
                .unwrap();
            let mut stream = response.into_inner();
            let err = stream.next().await.unwrap().unwrap_err();
            assert_eq!(err.code(), Code::ResourceExhausted);
        }

        /// An oversized error body degrades gracefully: the details are
        /// dropped but the code derived from the HTTP status is delivered.
        #[tokio::test]
        async fn test_unary_error_body_receive_max_bytes() {
            let app = Router::new().route(
                "/test.Service/Unary",
                post(|| async {
                    let body = format!(
                        r#"{{"code":"unauthenticated","message":"{}"}}"#,
                        "a".repeat(64 * 1024)
                    );
                    http::Response::builder()
                        .status(401)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap()
                }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .receive_max_bytes(1024)
                .build()
                .unwrap();

            let err = client
                .call_unary::<TestMessage, TestMessage>(
                    "test.Service/Unary",
                    &TestMessage::default(),
                )
                .await
                .unwrap_err();
            // 401 maps to Unauthenticated per connect-go's httpToCode
            assert_eq!(err.code(), Code::Unauthenticated);
        }

        /// An error body within the limit still parses fully.
        #[tokio::test]
        async fn test_unary_error_body_within_receive_max_bytes() {
            let app = Router::new().route(
                "/test.Service/Unary",
                post(|| async {
                    http::Response::builder()
                        .status(401)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(
                            r#"{"code":"unauthenticated","message":"nope"}"#,
                        ))
                        .unwrap()
                }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .receive_max_bytes(1024)
                .build()
                .unwrap();

            let err = client
                .call_unary::<TestMessage, TestMessage>(
                    "test.Service/Unary",
                    &TestMessage::default(),
                )
                .await
                .unwrap_err();
            assert_eq!(err.code(), Code::Unauthenticated);
            assert_eq!(err.message(), Some("nope"));
        }

        /// An oversized error body on a streaming call also degrades to the
        /// HTTP-status-derived code.
        #[tokio::test]
        async fn test_server_stream_error_body_receive_max_bytes() {
            let app = Router::new().route(
                "/test.Service/ServerStream",
                post(|| async {
                    let body = format!(
                        r#"{{"code":"unauthenticated","message":"{}"}}"#,
                        "a".repeat(64 * 1024)
                    );
                    http::Response::builder()
                        .status(401)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap()
                }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url)
                .receive_max_bytes(1024)
                .build()
                .unwrap();

            let err = match client
                .call_server_stream::<TestMessage, TestMessage>(
                    "test.Service/ServerStream",
                    &TestMessage::default(),
                )
                .await
            {
                Err(err) => err,
                Ok(_) => panic!("expected error"),
            };
            assert_eq!(err.code(), Code::Unauthenticated);
        }

        /// Multi-valued custom headers reach the server intact on streaming
        /// calls (insert-then-append semantics).
        #[tokio::test]
        async fn test_streaming_multi_value_headers_preserved() {
            let received = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
            let received_clone = received.clone();
            let app = Router::new().route(
                "/test.Service/ServerStream",
                post(move |req: http::Request<axum::body::Body>| {
                    let received = received_clone.clone();
                    async move {
                        let values: Vec<String> = req
                            .headers()
                            .get_all("x-tag")
                            .iter()
                            .map(|v| v.to_str().unwrap().to_string())
                            .collect();
                        *received.lock().unwrap() = values;
                        http::Response::builder()
                            .status(200)
                            .header(header::CONTENT_TYPE, "application/connect+json")
                            .body(axum::body::Body::from(streaming_response_body(
                                br#"{"value":"ok"}"#,
                            )))
                            .unwrap()
                    }
                }),
            );
            let base_url = spawn_server(app).await;

            let client = ConnectClient::builder(&base_url).build().unwrap();

            let mut options = crate::config::CallOptions::new();
            options.headers_mut().append("x-tag", "a".parse().unwrap());
            options.headers_mut().append("x-tag", "b".parse().unwrap());

            client
                .call_server_stream_with_options::<TestMessage, TestMessage>(
                    "test.Service/ServerStream",
                    &TestMessage::default(),
                    options,
                )
                .await
                .unwrap();

            assert_eq!(*received.lock().unwrap(), vec!["a", "b"]);
        }

        /// A bidi call over HTTP/1.1 reports the server's error rather than
        /// masking it with the HTTP/2 requirement.
        #[tokio::test]
        async fn test_bidi_http_status_error_not_masked_by_version_check() {
            let app = Router::new().route(
                "/test.Service/BidiStream",
                post(|| async {
                    http::Response::builder()
                        .status(401)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(
                            r#"{"code":"unauthenticated","message":"nope"}"#,
                        ))
                        .unwrap()
                }),
            );
            let base_url = spawn_server(app).await;

            // Default transport speaks HTTP/1.1 to the plaintext server.
            let client = ConnectClient::builder(&base_url).build().unwrap();

            let messages = futures::stream::iter(vec![TestMessage::default()]);
            let err = match client
                .call_bidi_stream::<TestMessage, TestMessage, _>(
                    "test.Service/BidiStream",
                    messages,
                )
                .await
            {
                Err(err) => err,
                Ok(_) => panic!("expected error"),
            };
            assert_eq!(err.code(), Code::Unauthenticated);
            assert_eq!(err.message(), Some("nope"));
        }
    }
}
