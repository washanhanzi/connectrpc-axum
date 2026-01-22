//! Connect RPC client implementation.
//!
//! This module provides the main [`ConnectClient`] type for making RPC calls.

use bytes::Bytes;

#[cfg(feature = "tracing")]
use tracing::info_span;
use connectrpc_axum_core::{CompressionConfig, CompressionEncoding, ConnectError};
use futures::{Stream, StreamExt};
use prost::Message;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

use crate::builder::ClientBuilder;
use crate::error_parser::parse_error_response;
use crate::frame::{FrameDecoder, FrameEncoder};
use crate::options::{duration_to_timeout_header, CallOptions};
use crate::response::{ConnectResponse, Metadata};
use crate::stream_body::StreamBody;

/// Header name for Connect protocol version.
const CONNECT_PROTOCOL_VERSION_HEADER: &str = "connect-protocol-version";

/// Connect protocol version.
const CONNECT_PROTOCOL_VERSION: &str = "1";

/// Header name for Connect timeout in milliseconds.
const CONNECT_TIMEOUT_HEADER: &str = "connect-timeout-ms";

/// Connect RPC client.
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
pub struct ConnectClient {
    /// HTTP client with middleware support.
    http: ClientWithMiddleware,
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
}

impl ConnectClient {
    /// Create a new ClientBuilder with the given base URL.
    ///
    /// This is a convenience method equivalent to `ClientBuilder::new(base_url)`.
    pub fn builder<S: Into<String>>(base_url: S) -> ClientBuilder {
        ClientBuilder::new(base_url)
    }

    /// Create a new ConnectClient.
    ///
    /// This is called by [`ClientBuilder::build`]. Prefer using the builder API.
    pub(crate) fn new(
        http: ClientWithMiddleware,
        base_url: String,
        use_proto: bool,
        compression: CompressionConfig,
        request_encoding: CompressionEncoding,
        accept_encoding: Option<CompressionEncoding>,
        default_timeout: Option<Duration>,
    ) -> Self {
        Self {
            http,
            base_url,
            use_proto,
            compression,
            request_encoding,
            accept_encoding,
            default_timeout,
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
        if self.use_proto {
            "proto"
        } else {
            "json"
        }
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
    fn encode_message<T>(&self, msg: &T) -> Result<Bytes, ConnectError>
    where
        T: Message + Serialize,
    {
        if self.use_proto {
            Ok(Bytes::from(msg.encode_to_vec()))
        } else {
            serde_json::to_vec(msg)
                .map(Bytes::from)
                .map_err(|e| ConnectError::Encode(format!("JSON encoding failed: {}", e)))
        }
    }

    /// Decode a message from response bytes.
    fn decode_message<T>(&self, bytes: &[u8]) -> Result<T, ConnectError>
    where
        T: Message + DeserializeOwned + Default,
    {
        if self.use_proto {
            T::decode(bytes).map_err(|e| ConnectError::Decode(format!("protobuf decoding failed: {}", e)))
        } else {
            serde_json::from_slice(bytes)
                .map_err(|e| ConnectError::Decode(format!("JSON decoding failed: {}", e)))
        }
    }

    /// Compress request body if configured.
    fn maybe_compress(&self, body: Bytes) -> Result<(Bytes, bool), ConnectError> {
        // Check if compression is enabled and body meets threshold
        if self.request_encoding.is_identity() || self.compression.is_disabled() {
            return Ok((body, false));
        }

        if body.len() < self.compression.min_bytes {
            return Ok((body, false));
        }

        // Get codec for the encoding
        let Some(codec) = self.request_encoding.codec_with_level(self.compression.level) else {
            return Ok((body, false));
        };

        // Compress
        let compressed = codec
            .compress(&body)
            .map_err(|e| ConnectError::Encode(format!("compression failed: {}", e)))?;

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
    /// Returns a [`ConnectError`] if:
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
    ) -> Result<ConnectResponse<Res>, ConnectError>
    where
        Req: Message + Serialize,
        Res: Message + DeserializeOwned + Default,
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

        // 1. Encode request body
        let body = self.encode_message(request)?;

        // 2. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // 3. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 4. Build request
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.unary_content_type());

        // Add Content-Encoding if compressed
        if compressed {
            req_builder =
                req_builder.header(reqwest::header::CONTENT_ENCODING, self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header(reqwest::header::ACCEPT_ENCODING, accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(timeout) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            // Parse error response
            return Err(parse_error_response(response).await);
        }

        // 7. Handle response decompression
        let content_encoding = headers
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding)
            .ok_or_else(|| {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get response body
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| ConnectError::Transport(format!("failed to read response body: {}", e)))?;

        // 9. Decompress if needed
        let body_bytes = if let Some(codec) = response_encoding.codec() {
            codec
                .decompress(&body_bytes)
                .map_err(|e| ConnectError::Decode(format!("decompression failed: {}", e)))?
        } else {
            body_bytes
        };

        // 10. Decode response
        let message = self.decode_message(&body_bytes)?;

        // 11. Extract metadata
        let metadata = Metadata::new(headers);

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
    ) -> Result<ConnectResponse<Res>, ConnectError>
    where
        Req: Message + Serialize,
        Res: Message + DeserializeOwned + Default,
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

        // 1. Encode request body
        let body = self.encode_message(request)?;

        // 2. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // 3. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 4. Build request
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.unary_content_type());

        // Add Content-Encoding if compressed
        if compressed {
            req_builder =
                req_builder.header(reqwest::header::CONTENT_ENCODING, self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header(reqwest::header::ACCEPT_ENCODING, accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(timeout) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Add custom headers from options
        for (name, value) in options.headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            // Parse error response
            return Err(parse_error_response(response).await);
        }

        // 7. Handle response decompression
        let content_encoding = headers
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding)
            .ok_or_else(|| {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get response body
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| ConnectError::Transport(format!("failed to read response body: {}", e)))?;

        // 9. Decompress if needed
        let body_bytes = if let Some(codec) = response_encoding.codec() {
            codec
                .decompress(&body_bytes)
                .map_err(|e| ConnectError::Decode(format!("decompression failed: {}", e)))?
        } else {
            body_bytes
        };

        // 10. Decode response
        let message = self.decode_message(&body_bytes)?;

        // 11. Extract metadata
        let metadata = Metadata::new(headers);

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
    /// Returns a [`ConnectResponse`] containing a [`StreamBody`] that yields
    /// response messages. After the stream is consumed, trailers are available
    /// via `stream.trailers()`.
    ///
    /// # Errors
    ///
    /// Returns a [`ConnectError`] if:
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
    ) -> Result<ConnectResponse<StreamBody<FrameDecoder<impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin, Res>>>, ConnectError>
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

        // 3. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 4. Build request with streaming content-type
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compressed
        if compressed {
            req_builder =
                req_builder.header(reqwest::header::CONTENT_ENCODING, self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(timeout) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            // Parse error response
            return Err(parse_error_response(response).await);
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        // For streaming, the encoding is per-frame and signaled via this header
        let content_encoding = headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding)
            .ok_or_else(|| {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get the streaming body
        let byte_stream = response.bytes_stream();

        // 9. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 10. Wrap with StreamBody
        let stream_body = StreamBody::new(decoder);

        // 11. Extract metadata from initial response headers
        let metadata = Metadata::new(headers);

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
            StreamBody<FrameDecoder<impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin, Res>>,
        >,
        ConnectError,
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

        // 1. Encode request body (for server streaming, request is NOT envelope-wrapped)
        let body = self.encode_message(request)?;

        // 2. Maybe compress
        let (body, compressed) = self.maybe_compress(body)?;

        // 3. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 4. Build request with streaming content-type
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compressed
        if compressed {
            req_builder =
                req_builder.header(reqwest::header::CONTENT_ENCODING, self.request_encoding.as_str());
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(timeout) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Add custom headers from options
        for (name, value) in options.headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            // Parse error response
            return Err(parse_error_response(response).await);
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding)
            .ok_or_else(|| {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            })?;

        // 8. Get the streaming body
        let byte_stream = response.bytes_stream();

        // 9. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 10. Wrap with StreamBody
        let stream_body = StreamBody::new(decoder);

        // 11. Extract metadata from initial response headers
        let metadata = Metadata::new(headers);

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
    /// Returns a [`ConnectError`] if:
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
    ) -> Result<ConnectResponse<Res>, ConnectError>
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

        // 1. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        // 3. Create streaming body from encoder
        // Map FrameEncoder's Result<Bytes, ConnectError> to reqwest's expected Result<Bytes, io::Error>
        let body_stream = futures::StreamExt::map(encoder, |result| {
            result.map_err(|e| std::io::Error::other(e.to_string()))
        });
        let body = reqwest::Body::wrap_stream(body_stream);

        // 4. Build request with streaming content-type
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder = req_builder.header(
                "connect-content-encoding",
                self.request_encoding.as_str(),
            );
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(timeout) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            return Err(parse_error_response(response).await);
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding).ok_or_else(
            || {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            },
        )?;

        // 8. Get the streaming body and decode the single response message
        let byte_stream = response.bytes_stream();
        let mut decoder = FrameDecoder::<_, Res>::new(byte_stream, self.use_proto, response_encoding);

        // 9. Get the single response message
        use futures::StreamExt;
        let message = decoder
            .next()
            .await
            .ok_or_else(|| ConnectError::Protocol("expected response message but stream ended".into()))??;

        // 10. Consume the EndStream frame to complete the stream
        // The decoder will return None after processing EndStream
        while decoder.next().await.is_some() {
            // Consume any remaining frames (there shouldn't be any after the message)
        }

        // 11. Extract metadata from response headers
        let metadata = Metadata::new(headers);

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
    ) -> Result<ConnectResponse<Res>, ConnectError>
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

        // 1. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression.clone(),
        );

        // 3. Create streaming body
        let body = reqwest::Body::wrap_stream(encoder);

        // 4. Build request with streaming content-type
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder = req_builder.header(
                "connect-content-encoding",
                self.request_encoding.as_str(),
            );
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(timeout) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Add custom headers from options
        for (name, value) in options.headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            return Err(parse_error_response(response).await);
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding).ok_or_else(
            || {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            },
        )?;

        // 8. Get the streaming body and decode the single response message
        let byte_stream = response.bytes_stream();
        let mut decoder = FrameDecoder::<_, Res>::new(byte_stream, self.use_proto, response_encoding);

        // 9. Get the single response message
        let message = match decoder.next().await {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => return Err(e),
            None => {
                return Err(ConnectError::Protocol(
                    "expected response message but stream ended".to_string(),
                ))
            }
        };

        // 10. Consume the EndStream frame to complete the stream
        while decoder.next().await.is_some() {
            // Consume any remaining frames
        }

        // 11. Extract metadata from response headers
        let metadata = Metadata::new(headers);

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
    /// Returns a [`ConnectResponse`] containing a [`StreamBody`] that yields
    /// response messages. After the stream is consumed, trailers are available
    /// via `stream.trailers()`.
    ///
    /// # Errors
    ///
    /// Returns a [`ConnectError`] if:
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
            StreamBody<FrameDecoder<impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin, Res>>,
        >,
        ConnectError,
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

        // 1. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression,
        );

        // 3. Create streaming body from encoder
        // Map FrameEncoder's Result<Bytes, ConnectError> to reqwest's expected Result<Bytes, io::Error>
        let body_stream = futures::StreamExt::map(encoder, |result| {
            result.map_err(|e| std::io::Error::other(e.to_string()))
        });
        let body = reqwest::Body::wrap_stream(body_stream);

        // 4. Build request with streaming content-type
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder = req_builder.header(
                "connect-content-encoding",
                self.request_encoding.as_str(),
            );
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header if timeout is configured
        if let Some(timeout) = self.default_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            return Err(parse_error_response(response).await);
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding).ok_or_else(
            || {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            },
        )?;

        // 8. Get the streaming body
        let byte_stream = response.bytes_stream();

        // 9. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 10. Wrap with StreamBody
        let stream_body = StreamBody::new(decoder);

        // 11. Extract metadata from initial response headers
        let metadata = Metadata::new(headers);

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
            StreamBody<FrameDecoder<impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin, Res>>,
        >,
        ConnectError,
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

        // 1. Build URL
        let url = format!("{}/{}", self.base_url, procedure);

        // 2. Wrap request stream with FrameEncoder
        let encoder = FrameEncoder::new(
            request,
            self.use_proto,
            self.request_encoding,
            self.compression.clone(),
        );

        // 3. Create streaming body
        let body = reqwest::Body::wrap_stream(encoder);

        // 4. Build request with streaming content-type
        let mut req_builder = self
            .http
            .post(&url)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION)
            .header(reqwest::header::CONTENT_TYPE, self.streaming_content_type());

        // Add Content-Encoding if compression is configured
        if !self.request_encoding.is_identity() && !self.compression.is_disabled() {
            req_builder = req_builder.header(
                "connect-content-encoding",
                self.request_encoding.as_str(),
            );
        }

        // Add Accept-Encoding if configured
        if let Some(accept) = &self.accept_encoding {
            req_builder = req_builder.header("connect-accept-encoding", accept.as_str());
        }

        // Add Connect-Timeout-Ms header (options timeout overrides default)
        let effective_timeout = options.timeout.or(self.default_timeout);
        if let Some(timeout) = effective_timeout {
            if let Some(timeout_ms) = duration_to_timeout_header(timeout) {
                req_builder = req_builder.header(CONNECT_TIMEOUT_HEADER, timeout_ms);
            }
        }

        // Add custom headers from options
        for (name, value) in options.headers.iter() {
            req_builder = req_builder.header(name, value);
        }

        // Set body
        req_builder = req_builder.body(body);

        // 5. Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("request failed: {}", e)))?;

        // 6. Check response status
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            return Err(parse_error_response(response).await);
        }

        // 7. Get compression encoding from Connect-Content-Encoding header
        let content_encoding = headers
            .get("connect-content-encoding")
            .and_then(|v| v.to_str().ok());

        let response_encoding = CompressionEncoding::from_header(content_encoding).ok_or_else(
            || {
                ConnectError::Protocol(format!(
                    "unsupported response encoding: {:?}",
                    content_encoding
                ))
            },
        )?;

        // 8. Get the streaming body
        let byte_stream = response.bytes_stream();

        // 9. Wrap with FrameDecoder
        let decoder = FrameDecoder::new(byte_stream, self.use_proto, response_encoding);

        // 10. Wrap with StreamBody
        let stream_body = StreamBody::new(decoder);

        // 11. Extract metadata from initial response headers
        let metadata = Metadata::new(headers);

        Ok(ConnectResponse::new(stream_body, metadata))
    }
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
