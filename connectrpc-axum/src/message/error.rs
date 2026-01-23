//! Server-side Connect error types and response encoding.
//!
//! This module provides the server-side [`ConnectError`] type with HTTP response
//! generation capabilities, plus helper functions for building streaming error frames.

use axum::{
    Json,
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Serialize, Serializer};
use std::collections::HashMap;

use crate::context::RequestProtocol;

// Re-export core types
pub use connectrpc_axum_core::{Code, ErrorDetail};

// ============================================================================
// ConnectError - Server-side error with HTTP response generation
// ============================================================================

/// An error that captures the key pieces of information for Connect RPC:
/// a code, an optional message, metadata (HTTP headers), and optional error details.
#[derive(Clone, Debug)]
pub struct ConnectError {
    code: Code,
    message: Option<String>,
    details: Vec<ErrorDetail>,
    meta: Option<HeaderMap>,
}

impl ConnectError {
    /// Create a new error with a code and message.
    pub fn new<S: Into<String>>(code: Code, message: S) -> Self {
        Self {
            code,
            message: Some(message.into()),
            details: vec![],
            meta: None,
        }
    }

    /// Create a new error with just a code.
    pub fn from_code(code: Code) -> Self {
        Self {
            code,
            message: None,
            details: vec![],
            meta: None,
        }
    }

    /// Create an unimplemented error.
    pub fn new_unimplemented() -> Self {
        Self {
            code: Code::Unimplemented,
            message: Some("The requested service has not been implemented.".to_string()),
            details: vec![],
            meta: None,
        }
    }

    /// Create an invalid argument error.
    pub fn new_invalid_argument<S: Into<String>>(message: S) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    /// Create a not found error.
    pub fn new_not_found<S: Into<String>>(message: S) -> Self {
        Self::new(Code::NotFound, message)
    }

    /// Create a permission denied error.
    pub fn new_permission_denied<S: Into<String>>(message: S) -> Self {
        Self::new(Code::PermissionDenied, message)
    }

    /// Create an unauthenticated error.
    pub fn new_unauthenticated<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unauthenticated, message)
    }

    /// Create an internal error.
    pub fn new_internal<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Internal, message)
    }

    /// Create an unavailable error.
    pub fn new_unavailable<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unavailable, message)
    }

    /// Create an already exists error.
    pub fn new_already_exists<S: Into<String>>(message: S) -> Self {
        Self::new(Code::AlreadyExists, message)
    }

    /// Create a resource exhausted error.
    pub fn new_resource_exhausted<S: Into<String>>(message: S) -> Self {
        Self::new(Code::ResourceExhausted, message)
    }

    /// Create a failed precondition error.
    pub fn new_failed_precondition<S: Into<String>>(message: S) -> Self {
        Self::new(Code::FailedPrecondition, message)
    }

    /// Create an aborted error.
    pub fn new_aborted<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Aborted, message)
    }

    /// Get the error code.
    pub fn code(&self) -> Code {
        self.code
    }

    /// Get the error message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Get the error details.
    pub fn details(&self) -> &[ErrorDetail] {
        &self.details
    }

    /// Add an error detail with type URL and protobuf-encoded bytes.
    pub fn add_detail<S: Into<String>>(mut self, type_url: S, value: Vec<u8>) -> Self {
        self.details.push(ErrorDetail::new(type_url, value));
        self
    }

    /// Add a pre-constructed ErrorDetail.
    pub fn add_error_detail(mut self, detail: ErrorDetail) -> Self {
        self.details.push(detail);
        self
    }

    /// Get the metadata headers, if any.
    pub fn meta(&self) -> Option<&HeaderMap> {
        self.meta.as_ref()
    }

    /// Get mutable access to metadata headers.
    /// Lazily initializes the HeaderMap if not present.
    pub fn meta_mut(&mut self) -> &mut HeaderMap {
        self.meta.get_or_insert_with(HeaderMap::new)
    }

    /// Add a metadata header.
    pub fn with_meta<K, V>(mut self, key: K, value: V) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let key_str = key.as_ref();
        let val_str = value.as_ref();

        match HeaderName::from_bytes(key_str.as_bytes()) {
            Ok(name) => match HeaderValue::from_str(val_str) {
                Ok(val) => {
                    self.meta_mut().append(name, val);
                }
                Err(e) => {
                    tracing::debug!(
                        key = key_str,
                        value = val_str,
                        error = %e,
                        "invalid header value, metadata dropped"
                    );
                }
            },
            Err(e) => {
                tracing::debug!(
                    key = key_str,
                    error = %e,
                    "invalid header name, metadata dropped"
                );
            }
        }
        self
    }

    /// Set metadata from HeaderMap.
    pub fn set_meta_from_headers(mut self, headers: &HeaderMap) -> Self {
        self.meta = Some(headers.clone());
        self
    }
}

impl ConnectError {
    /// Convert this error into an HTTP response using the specified protocol.
    ///
    /// This is the primary method used by handler wrappers to convert errors
    /// to responses with the correct encoding based on the request protocol.
    pub(crate) fn into_response_with_protocol(self, protocol: RequestProtocol) -> Response {
        // For streaming protocols, errors must be returned as EndStream frames
        // with HTTP 200, not as HTTP error status codes
        if protocol.is_streaming() {
            return self.into_streaming_error_response(protocol);
        }

        // For unary protocols, use HTTP status codes
        let status_code = self.http_status_code();

        // Create the error response body
        let error_body = ErrorResponseBody {
            code: self.code,
            message: self.message,
            details: self.details,
        };

        // Start with the base response
        let mut response = (status_code, Json(error_body)).into_response();

        // Set the correct content-type for errors
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(protocol.error_content_type()),
        );

        // Add metadata as headers
        if let Some(meta) = &self.meta {
            let headers = response.headers_mut();
            headers.extend(meta.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        response
    }
}

impl IntoResponse for ConnectError {
    fn into_response(self) -> Response {
        // Fallback to default protocol (ConnectUnaryJson)
        // Handler wrappers should use into_response_with_protocol() instead
        self.into_response_with_protocol(RequestProtocol::default())
    }
}

impl ConnectError {
    /// Convert error code to HTTP status code (for unary responses only)
    fn http_status_code(&self) -> StatusCode {
        match self.code {
            Code::Ok => StatusCode::OK,
            // 499 Client Closed Request (nginx extension) - client canceled the operation
            Code::Canceled => StatusCode::from_u16(499).unwrap(),
            Code::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
            Code::InvalidArgument => StatusCode::BAD_REQUEST,
            // 504 Gateway Timeout - server-side deadline exceeded
            Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
            Code::NotFound => StatusCode::NOT_FOUND,
            Code::AlreadyExists => StatusCode::CONFLICT,
            Code::PermissionDenied => StatusCode::FORBIDDEN,
            Code::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
            Code::FailedPrecondition => StatusCode::BAD_REQUEST,
            Code::Aborted => StatusCode::CONFLICT,
            Code::OutOfRange => StatusCode::BAD_REQUEST,
            Code::Unimplemented => StatusCode::NOT_IMPLEMENTED,
            Code::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Code::DataLoss => StatusCode::INTERNAL_SERVER_ERROR,
            Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        }
    }

    /// Create a streaming error response with proper EndStream framing.
    ///
    /// Per the Connect protocol, streaming responses must:
    /// - Always return HTTP 200
    /// - Use application/connect+json or application/connect+proto content-type
    /// - Deliver errors in an EndStream frame (flags = 0x02)
    pub fn into_streaming_response(self, use_proto: bool) -> Response {
        let content_type = if use_proto {
            "application/connect+proto"
        } else {
            "application/connect+json"
        };
        self.into_streaming_error_response_with_content_type(content_type)
    }

    /// Internal helper for creating streaming error responses.
    fn into_streaming_error_response(self, protocol: RequestProtocol) -> Response {
        self.into_streaming_error_response_with_content_type(protocol.error_content_type())
    }

    /// Create a streaming error response with the specified content-type.
    fn into_streaming_error_response_with_content_type(
        self,
        content_type: &'static str,
    ) -> Response {
        // Use build_end_stream_frame which properly includes error metadata in the
        // EndStream JSON payload's "metadata" field (per Connect protocol spec)
        let frame = build_end_stream_frame(Some(&self), None);

        // Build the response with HTTP 200 and streaming content-type
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
            .body(Body::from(frame))
            .unwrap_or_else(|_| internal_error_streaming_response(content_type))
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl From<std::convert::Infallible> for ConnectError {
    fn from(infallible: std::convert::Infallible) -> Self {
        match infallible {}
    }
}

impl From<(StatusCode, String)> for ConnectError {
    /// Convert an HTTP status code and message into a ConnectError.
    fn from(value: (StatusCode, String)) -> Self {
        let (status, message) = value;
        ConnectError::new(code_from_status(status), message)
    }
}

/// Convert an HTTP status code to a Connect error code.
///
/// This is used when translating HTTP errors to Connect errors.
pub fn code_from_status(status: StatusCode) -> Code {
    match status {
        StatusCode::OK => Code::Ok,
        StatusCode::BAD_REQUEST => Code::InvalidArgument,
        StatusCode::UNAUTHORIZED => Code::Unauthenticated,
        StatusCode::FORBIDDEN => Code::PermissionDenied,
        StatusCode::NOT_FOUND => Code::NotFound,
        StatusCode::CONFLICT => Code::AlreadyExists,
        StatusCode::REQUEST_TIMEOUT => Code::DeadlineExceeded,
        StatusCode::TOO_MANY_REQUESTS => Code::ResourceExhausted,
        StatusCode::NOT_IMPLEMENTED => Code::Unimplemented,
        StatusCode::SERVICE_UNAVAILABLE => Code::Unavailable,
        StatusCode::INTERNAL_SERVER_ERROR => Code::Internal,
        _ => Code::Unknown,
    }
}

// ============================================================================
// Serialization
// ============================================================================

/// The JSON body structure for error responses.
#[derive(Serialize)]
struct ErrorResponseBody {
    code: Code,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    details: Vec<ErrorDetail>,
}

impl Serialize for ConnectError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize only the parts that should go in the JSON body
        ErrorResponseBody {
            code: self.code,
            message: self.message.clone(),
            details: self.details.clone(),
        }
        .serialize(serializer)
    }
}

// ============================================================================
// Tonic Conversions (feature-gated)
// ============================================================================

/// Convert a tonic Code to a Connect Code.
#[cfg(feature = "tonic")]
pub fn code_from_tonic(code: ::tonic::Code) -> Code {
    match code {
        ::tonic::Code::Ok => Code::Ok,
        ::tonic::Code::Cancelled => Code::Canceled,
        ::tonic::Code::Unknown => Code::Unknown,
        ::tonic::Code::InvalidArgument => Code::InvalidArgument,
        ::tonic::Code::DeadlineExceeded => Code::DeadlineExceeded,
        ::tonic::Code::NotFound => Code::NotFound,
        ::tonic::Code::AlreadyExists => Code::AlreadyExists,
        ::tonic::Code::PermissionDenied => Code::PermissionDenied,
        ::tonic::Code::ResourceExhausted => Code::ResourceExhausted,
        ::tonic::Code::FailedPrecondition => Code::FailedPrecondition,
        ::tonic::Code::Aborted => Code::Aborted,
        ::tonic::Code::OutOfRange => Code::OutOfRange,
        ::tonic::Code::Unimplemented => Code::Unimplemented,
        ::tonic::Code::Internal => Code::Internal,
        ::tonic::Code::Unavailable => Code::Unavailable,
        ::tonic::Code::DataLoss => Code::DataLoss,
        ::tonic::Code::Unauthenticated => Code::Unauthenticated,
    }
}

/// Convert a Connect Code to a tonic Code.
#[cfg(feature = "tonic")]
pub fn code_to_tonic(code: Code) -> ::tonic::Code {
    match code {
        Code::Ok => ::tonic::Code::Ok,
        Code::Canceled => ::tonic::Code::Cancelled,
        Code::Unknown => ::tonic::Code::Unknown,
        Code::InvalidArgument => ::tonic::Code::InvalidArgument,
        Code::DeadlineExceeded => ::tonic::Code::DeadlineExceeded,
        Code::NotFound => ::tonic::Code::NotFound,
        Code::AlreadyExists => ::tonic::Code::AlreadyExists,
        Code::PermissionDenied => ::tonic::Code::PermissionDenied,
        Code::ResourceExhausted => ::tonic::Code::ResourceExhausted,
        Code::FailedPrecondition => ::tonic::Code::FailedPrecondition,
        Code::Aborted => ::tonic::Code::Aborted,
        Code::OutOfRange => ::tonic::Code::OutOfRange,
        Code::Unimplemented => ::tonic::Code::Unimplemented,
        Code::Internal => ::tonic::Code::Internal,
        Code::Unavailable => ::tonic::Code::Unavailable,
        Code::DataLoss => ::tonic::Code::DataLoss,
        Code::Unauthenticated => ::tonic::Code::Unauthenticated,
    }
}

#[cfg(feature = "tonic")]
impl From<::tonic::Status> for ConnectError {
    fn from(status: ::tonic::Status) -> Self {
        let code = code_from_tonic(status.code());
        let msg = status.message().to_string();

        if msg.is_empty() {
            ConnectError::from_code(code)
        } else {
            ConnectError::new(code, msg)
        }
    }
}

#[cfg(feature = "tonic")]
impl From<ConnectError> for ::tonic::Status {
    fn from(err: ConnectError) -> Self {
        let code = code_to_tonic(err.code());
        ::tonic::Status::new(code, err.message().unwrap_or("").to_string())
    }
}

// ============================================================================
// EndStream Metadata Support
// ============================================================================

/// Check if a header key is a protocol header that should be filtered from metadata.
///
/// Protocol headers are internal to HTTP/Connect/gRPC and should not be included
/// in the metadata field of EndStream messages.
fn is_protocol_header(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    matches!(
        k.as_str(),
        "content-type"
            | "content-length"
            | "content-encoding"
            | "host"
            | "user-agent"
            | "trailer"
            | "date"
    ) || k.starts_with("connect-")
        || k.starts_with("grpc-")
        || k.starts_with("trailer-")
}

/// Metadata wrapper for EndStream messages.
///
/// Serializes HTTP headers to Connect protocol metadata format:
/// - Keys map to arrays of string values
/// - Binary headers (keys ending in `-bin`) have base64-encoded values
/// - Protocol headers are filtered out
#[derive(Debug, Default)]
pub struct Metadata(HashMap<String, Vec<String>>);

impl Metadata {
    /// Create Metadata from a HeaderMap, filtering protocol headers
    /// and encoding binary values.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();

        for (key, value) in headers.iter() {
            let key_str = key.as_str();

            // Skip protocol headers
            if is_protocol_header(key_str) {
                continue;
            }

            let values = map.entry(key_str.to_string()).or_default();

            // For -bin headers, values are already base64-encoded per Connect/gRPC convention.
            // Just convert to string (no re-encoding needed).
            // For regular headers, convert to UTF-8 string.
            if let Ok(v) = value.to_str() {
                values.push(v.to_string());
            }
            // Skip non-UTF8 values (shouldn't happen with valid HTTP headers)
        }

        Metadata(map)
    }

    /// Merge headers from another HeaderMap into this metadata.
    pub fn merge_headers(&mut self, headers: &HeaderMap) {
        for (key, value) in headers.iter() {
            let key_str = key.as_str();

            if is_protocol_header(key_str) {
                continue;
            }

            let values = self.0.entry(key_str.to_string()).or_default();

            if let Ok(v) = value.to_str() {
                values.push(v.to_string());
            }
        }
    }

    /// Check if metadata is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Metadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

// ============================================================================
// EndStream Frame Building
// ============================================================================

/// Connect streaming envelope flags.
mod envelope_flags {
    /// End of stream.
    pub const END_STREAM: u8 = 0x02;
}

/// Build an EndStream frame for streaming responses.
///
/// Frame format: `[flags=0x02][length:4][json_payload]`
///
/// The JSON payload follows the Connect protocol specification:
/// ```json
/// {
///   "error": { "code": "...", "message": "...", "details": [...] },
///   "metadata": { "key": ["value1", "value2"] }
/// }
/// ```
/// Both fields are optional and omitted when empty/None.
pub fn build_end_stream_frame(
    error: Option<&ConnectError>,
    trailers: Option<&HeaderMap>,
) -> Vec<u8> {
    #[derive(Serialize)]
    struct EndStreamMessage<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a ConnectError>,
        #[serde(skip_serializing_if = "Metadata::is_empty")]
        metadata: Metadata,
    }

    // Start with trailers if provided
    let mut metadata = trailers.map(Metadata::from_headers).unwrap_or_default();

    // Merge error metadata into trailers (like connect-go does)
    if let Some(err) = error
        && let Some(meta) = err.meta()
    {
        metadata.merge_headers(meta);
    }

    let msg = EndStreamMessage { error, metadata };
    let payload = serde_json::to_vec(&msg).unwrap_or_else(|_| b"{}".to_vec());

    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(envelope_flags::END_STREAM);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame
}

// ============================================================================
// Safe Fallback Responses
// ============================================================================

/// Create a safe 500 Internal Server Error response for unary requests.
///
/// This is used when serialization or encoding fails and we cannot produce
/// a proper ConnectError response. The body is a hardcoded JSON string that
/// cannot fail to serialize.
pub(crate) fn internal_error_response(content_type: &'static str) -> Response {
    // Hardcoded JSON that cannot fail - no dynamic content
    const ERROR_BODY: &[u8] = br#"{"code":"internal","message":"Internal serialization error"}"#;

    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
        .body(Body::from(ERROR_BODY.to_vec()))
        // This cannot fail: status is valid, header is valid static strings, body is valid bytes
        .unwrap_or_else(|_| {
            // Ultimate fallback - empty 500 response
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            response
        })
}

/// Create a safe EndStream error frame for streaming responses.
///
/// This is used when encoding a message in a stream fails. Returns bytes
/// for an EndStream frame (flags=0x02) containing an internal error.
pub(crate) fn internal_error_end_stream_frame() -> Vec<u8> {
    // Hardcoded EndStream JSON payload that cannot fail
    const ERROR_PAYLOAD: &[u8] =
        br#"{"error":{"code":"internal","message":"Internal serialization error"}}"#;

    let mut frame = Vec::with_capacity(5 + ERROR_PAYLOAD.len());
    frame.push(0b0000_0010); // EndStream flag
    frame.extend_from_slice(&(ERROR_PAYLOAD.len() as u32).to_be_bytes());
    frame.extend_from_slice(ERROR_PAYLOAD);
    frame
}

/// Create a safe streaming response with an internal error EndStream frame.
///
/// This is used when we cannot build a proper streaming response and need
/// to return a safe fallback.
pub(crate) fn internal_error_streaming_response(content_type: &'static str) -> Response {
    let frame = internal_error_end_stream_frame();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
        .body(Body::from(frame))
        .unwrap_or_else(|_| {
            // Ultimate fallback - empty 200 response
            Response::new(Body::empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_error_new() {
        let err = ConnectError::new(Code::NotFound, "resource not found");
        assert!(matches!(err.code(), Code::NotFound));
        assert_eq!(err.message(), Some("resource not found"));
        assert!(err.details().is_empty());
        assert!(err.meta().is_none());
    }

    #[test]
    fn test_connect_error_from_code() {
        let err = ConnectError::from_code(Code::Internal);
        assert!(matches!(err.code(), Code::Internal));
        assert!(err.message().is_none());
    }

    #[test]
    fn test_with_meta_valid_headers() {
        let err = ConnectError::new(Code::Internal, "error")
            .with_meta("x-request-id", "req-123")
            .with_meta("x-trace-id", "trace-456");

        let meta = err.meta().expect("metadata should be present");
        assert_eq!(meta.get("x-request-id").unwrap(), "req-123");
        assert_eq!(meta.get("x-trace-id").unwrap(), "trace-456");
    }

    #[test]
    fn test_add_detail() {
        let err = ConnectError::new(Code::Internal, "error")
            .add_detail("test.Type1", vec![1, 2, 3])
            .add_detail("test.Type2", vec![4, 5, 6]);

        assert_eq!(err.details().len(), 2);
        assert_eq!(err.details()[0].type_url(), "test.Type1");
        assert_eq!(err.details()[0].value(), &[1, 2, 3]);
    }

    #[test]
    fn test_is_protocol_header_filters_http_headers() {
        assert!(is_protocol_header("Content-Type"));
        assert!(is_protocol_header("content-type"));
        assert!(is_protocol_header("Content-Length"));
    }

    #[test]
    fn test_is_protocol_header_filters_connect_headers() {
        assert!(is_protocol_header("Connect-Timeout-Ms"));
        assert!(is_protocol_header("connect-timeout-ms"));
    }

    #[test]
    fn test_is_protocol_header_allows_custom_headers() {
        assert!(!is_protocol_header("X-Custom-Header"));
        assert!(!is_protocol_header("x-request-id"));
    }

    #[test]
    fn test_metadata_from_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("x-custom", HeaderValue::from_static("value"));

        let metadata = Metadata::from_headers(&headers);

        assert!(!metadata.0.contains_key("content-type"));
        assert!(metadata.0.contains_key("x-custom"));
    }

    #[test]
    fn test_build_end_stream_frame_success() {
        let frame = build_end_stream_frame(None, None);

        assert_eq!(frame[0], 0x02); // EndStream flag

        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert_eq!(msg, serde_json::json!({}));
    }

    #[test]
    fn test_build_end_stream_frame_with_error() {
        let error = ConnectError::new(Code::Internal, "test error");
        let frame = build_end_stream_frame(Some(&error), None);

        assert_eq!(frame[0], 0x02);

        let payload = &frame[5..];
        let msg: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert_eq!(msg["error"]["code"], "internal");
        assert_eq!(msg["error"]["message"], "test error");
    }

    #[test]
    fn test_internal_error_end_stream_frame() {
        let frame = internal_error_end_stream_frame();

        assert!(frame.len() > 5);
        assert_eq!(frame[0], 0b0000_0010);

        let payload = &frame[5..];
        let parsed: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert_eq!(parsed["error"]["code"], "internal");
    }
}
