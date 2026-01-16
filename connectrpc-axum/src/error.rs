use axum::{
    Json,
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Serialize, Serializer};

use crate::context::RequestProtocol;

/// Connect RPC error codes, matching the codes defined in the Connect protocol.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    Ok = 0,
    Canceled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

/// A self-describing error detail following the Connect protocol.
///
/// Error details are structured Protobuf messages attached to errors,
/// allowing clients to receive strongly-typed error information.
/// This maps to `google.protobuf.Any` on the wire.
///
/// # Wire Format
///
/// Details are serialized as JSON objects with `type` and `value` fields:
/// ```json
/// {"type": "google.rpc.RetryInfo", "value": "base64-encoded-protobuf"}
/// ```
///
/// # Example
///
/// ```ignore
/// use prost::Message;
///
/// // Encode a google.rpc.RetryInfo message
/// let retry_delay = prost_types::Duration { seconds: 5, nanos: 0 };
/// let mut bytes = Vec::new();
/// // ... encode RetryInfo with retry_delay field
///
/// let detail = ErrorDetail::new("google.rpc.RetryInfo", bytes);
/// ```
#[derive(Clone, Debug)]
pub struct ErrorDetail {
    /// Fully-qualified type name (e.g., "google.rpc.RetryInfo").
    type_url: String,
    /// Protobuf-encoded message bytes.
    value: Vec<u8>,
}

impl ErrorDetail {
    /// Create a new error detail with a type URL and protobuf-encoded bytes.
    pub fn new<S: Into<String>>(type_url: S, value: Vec<u8>) -> Self {
        Self {
            type_url: type_url.into(),
            value,
        }
    }

    /// Get the fully-qualified type name.
    pub fn type_url(&self) -> &str {
        &self.type_url
    }

    /// Get the protobuf-encoded value bytes.
    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

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
    ///
    /// # Example
    ///
    /// ```ignore
    /// use prost::Message;
    ///
    /// let duration = prost_types::Duration { seconds: 5, nanos: 0 };
    /// let mut bytes = Vec::new();
    /// duration.encode(&mut bytes).unwrap();
    ///
    /// // Wrap in RetryInfo (field 1)
    /// let mut retry_info_bytes = vec![0x0a, bytes.len() as u8];
    /// retry_info_bytes.extend(bytes);
    ///
    /// let err = ConnectError::new(Code::ResourceExhausted, "rate limited")
    ///     .add_detail("google.rpc.RetryInfo", retry_info_bytes);
    /// ```
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
            Code::Canceled => StatusCode::REQUEST_TIMEOUT,
            Code::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
            Code::InvalidArgument => StatusCode::BAD_REQUEST,
            Code::DeadlineExceeded => StatusCode::REQUEST_TIMEOUT,
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
    ///
    /// This method should be used by streaming handlers when returning errors
    /// before the stream has started. The `use_proto` flag determines the
    /// response encoding (protobuf vs JSON).
    pub fn into_streaming_response(self, use_proto: bool) -> Response {
        let content_type = if use_proto {
            "application/connect+proto"
        } else {
            "application/connect+json"
        };
        self.into_streaming_error_response_with_content_type(content_type)
    }

    /// Internal helper for creating streaming error responses.
    fn into_streaming_error_response(self, protocol: crate::context::RequestProtocol) -> Response {
        self.into_streaming_error_response_with_content_type(protocol.error_content_type())
    }

    /// Create a streaming error response with the specified content-type.
    fn into_streaming_error_response_with_content_type(
        self,
        content_type: &'static str,
    ) -> Response {
        // Use build_end_stream_frame which properly includes error metadata in the
        // EndStream JSON payload's "metadata" field (per Connect protocol spec)
        let frame = crate::pipeline::build_end_stream_frame(Some(&self), None);

        // Build the response with HTTP 200 and streaming content-type
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
            .body(Body::from(frame))
            .unwrap_or_else(|_| internal_error_streaming_response(content_type))
    }
}

// ---- Conversions ----

impl From<std::convert::Infallible> for ConnectError {
    fn from(infallible: std::convert::Infallible) -> Self {
        match infallible {}
    }
}

impl From<(StatusCode, String)> for ConnectError {
    /// Convert an HTTP status code and message into a ConnectError.
    ///
    /// This provides a simple DX helper to lift common HTTP errors into
    /// Connect's error space.
    fn from(value: (StatusCode, String)) -> Self {
        let (status, message) = value;
        ConnectError::new(status.into(), message)
    }
}

impl From<StatusCode> for Code {
    fn from(status: StatusCode) -> Self {
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
}

/// The JSON body structure for error responses.
#[derive(Serialize)]
struct ErrorResponseBody {
    code: Code,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    details: Vec<ErrorDetail>,
}

impl Serialize for ErrorDetail {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use base64::Engine;
        use serde::ser::SerializeStruct;

        let mut s = serializer.serialize_struct("ErrorDetail", 2)?;

        // Strip "type.googleapis.com/" prefix if present (Connect uses short type names)
        let type_name = self
            .type_url
            .strip_prefix("type.googleapis.com/")
            .unwrap_or(&self.type_url);
        s.serialize_field("type", type_name)?;

        // Connect protocol uses raw base64 (no padding)
        let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&self.value);
        s.serialize_field("value", &encoded)?;

        s.end()
    }
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

// ---- Conversions from tonic types (feature-gated) ----
#[cfg(feature = "tonic")]
impl From<::tonic::Code> for Code {
    fn from(code: ::tonic::Code) -> Self {
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
}

#[cfg(feature = "tonic")]
impl From<::tonic::Status> for ConnectError {
    fn from(status: ::tonic::Status) -> Self {
        let code: Code = status.code().into();
        let msg = status.message().to_string();

        // Note: Tonic status can carry metadata, but Connect error metadata is HTTP headers.
        // We currently carry just code + message to align with Connect JSON shape.
        // Details are not directly accessible from `tonic::Status`.
        if msg.is_empty() {
            ConnectError::from_code(code)
        } else {
            ConnectError::new(code, msg)
        }
    }
}

#[cfg(feature = "tonic")]
impl From<Code> for ::tonic::Code {
    fn from(code: Code) -> Self {
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
}

#[cfg(feature = "tonic")]
impl From<ConnectError> for ::tonic::Status {
    fn from(err: ConnectError) -> Self {
        let code: ::tonic::Code = err.code().into();
        ::tonic::Status::new(code, err.message().unwrap_or("").to_string())
    }
}

// ---- Safe fallback responses for serialization/encoding failures ----

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
    use axum::http::HeaderMap;

    // ---- Code and ConnectError basic tests ----

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
    fn test_connect_error_convenience_constructors() {
        let err = ConnectError::new_unimplemented();
        assert!(matches!(err.code(), Code::Unimplemented));
        assert!(err.message().is_some());

        let err = ConnectError::new_invalid_argument("bad input");
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert_eq!(err.message(), Some("bad input"));

        let err = ConnectError::new_not_found("missing");
        assert!(matches!(err.code(), Code::NotFound));

        let err = ConnectError::new_permission_denied("forbidden");
        assert!(matches!(err.code(), Code::PermissionDenied));

        let err = ConnectError::new_unauthenticated("no auth");
        assert!(matches!(err.code(), Code::Unauthenticated));

        let err = ConnectError::new_internal("server error");
        assert!(matches!(err.code(), Code::Internal));

        let err = ConnectError::new_unavailable("try later");
        assert!(matches!(err.code(), Code::Unavailable));

        let err = ConnectError::new_already_exists("duplicate");
        assert!(matches!(err.code(), Code::AlreadyExists));

        let err = ConnectError::new_resource_exhausted("quota exceeded");
        assert!(matches!(err.code(), Code::ResourceExhausted));

        let err = ConnectError::new_failed_precondition("precondition failed");
        assert!(matches!(err.code(), Code::FailedPrecondition));

        let err = ConnectError::new_aborted("aborted");
        assert!(matches!(err.code(), Code::Aborted));
    }

    // ---- Metadata tests ----

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
    fn test_with_meta_invalid_header_name_is_dropped() {
        // Invalid header name (contains spaces) should be silently dropped
        let err = ConnectError::new(Code::Internal, "error")
            .with_meta("invalid header", "value")
            .with_meta("x-valid", "kept");

        let meta = err.meta().expect("metadata should be present");
        assert!(meta.get("invalid header").is_none());
        assert_eq!(meta.get("x-valid").unwrap(), "kept");
    }

    #[test]
    fn test_with_meta_invalid_header_value_is_dropped() {
        // Invalid header value (contains non-visible ASCII) should be silently dropped
        let err = ConnectError::new(Code::Internal, "error")
            .with_meta("x-bad-value", "value\x00with\x01null")
            .with_meta("x-valid", "kept");

        let meta = err.meta().expect("metadata should be present");
        assert!(meta.get("x-bad-value").is_none());
        assert_eq!(meta.get("x-valid").unwrap(), "kept");
    }

    #[test]
    fn test_with_meta_multiple_values_same_key() {
        let err = ConnectError::new(Code::Internal, "error")
            .with_meta("x-multi", "value1")
            .with_meta("x-multi", "value2");

        let meta = err.meta().expect("metadata should be present");
        let values: Vec<_> = meta.get_all("x-multi").iter().collect();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], "value1");
        assert_eq!(values[1], "value2");
    }

    #[test]
    fn test_meta_mut() {
        let mut err = ConnectError::new(Code::Internal, "error");

        // First call initializes the map
        err.meta_mut().insert(
            HeaderName::from_static("x-custom"),
            HeaderValue::from_static("value"),
        );

        assert!(err.meta().is_some());
        assert_eq!(err.meta().unwrap().get("x-custom").unwrap(), "value");

        // Second call returns the same map
        err.meta_mut().insert(
            HeaderName::from_static("x-another"),
            HeaderValue::from_static("value2"),
        );

        assert_eq!(err.meta().unwrap().len(), 2);
    }

    #[test]
    fn test_set_meta_from_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-from-map"),
            HeaderValue::from_static("map-value"),
        );
        headers.insert(
            HeaderName::from_static("x-another"),
            HeaderValue::from_static("another-value"),
        );

        let err = ConnectError::new(Code::Internal, "error").set_meta_from_headers(&headers);

        let meta = err.meta().expect("metadata should be present");
        assert_eq!(meta.get("x-from-map").unwrap(), "map-value");
        assert_eq!(meta.get("x-another").unwrap(), "another-value");
    }

    #[test]
    fn test_set_meta_from_headers_replaces_existing() {
        let err = ConnectError::new(Code::Internal, "error").with_meta("x-old", "old-value");

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-new"),
            HeaderValue::from_static("new-value"),
        );

        let err = err.set_meta_from_headers(&headers);

        let meta = err.meta().expect("metadata should be present");
        // Old metadata is replaced
        assert!(meta.get("x-old").is_none());
        assert_eq!(meta.get("x-new").unwrap(), "new-value");
    }

    // ---- Details tests ----

    #[test]
    fn test_add_detail() {
        let err = ConnectError::new(Code::Internal, "error")
            .add_detail("test.Type1", vec![1, 2, 3])
            .add_detail("test.Type2", vec![4, 5, 6]);

        assert_eq!(err.details().len(), 2);
        assert_eq!(err.details()[0].type_url(), "test.Type1");
        assert_eq!(err.details()[0].value(), &[1, 2, 3]);
        assert_eq!(err.details()[1].type_url(), "test.Type2");
        assert_eq!(err.details()[1].value(), &[4, 5, 6]);
    }

    #[test]
    fn test_add_error_detail() {
        let detail = ErrorDetail::new("google.rpc.RetryInfo", vec![10, 2, 8, 5]);
        let err =
            ConnectError::new(Code::ResourceExhausted, "rate limited").add_error_detail(detail);

        assert_eq!(err.details().len(), 1);
        assert_eq!(err.details()[0].type_url(), "google.rpc.RetryInfo");
    }

    // ---- HTTP status code mapping tests ----

    #[test]
    fn test_http_status_code_mapping() {
        let test_cases = [
            (Code::Ok, StatusCode::OK),
            (Code::Canceled, StatusCode::REQUEST_TIMEOUT),
            (Code::Unknown, StatusCode::INTERNAL_SERVER_ERROR),
            (Code::InvalidArgument, StatusCode::BAD_REQUEST),
            (Code::DeadlineExceeded, StatusCode::REQUEST_TIMEOUT),
            (Code::NotFound, StatusCode::NOT_FOUND),
            (Code::AlreadyExists, StatusCode::CONFLICT),
            (Code::PermissionDenied, StatusCode::FORBIDDEN),
            (Code::ResourceExhausted, StatusCode::TOO_MANY_REQUESTS),
            (Code::FailedPrecondition, StatusCode::BAD_REQUEST),
            (Code::Aborted, StatusCode::CONFLICT),
            (Code::OutOfRange, StatusCode::BAD_REQUEST),
            (Code::Unimplemented, StatusCode::NOT_IMPLEMENTED),
            (Code::Internal, StatusCode::INTERNAL_SERVER_ERROR),
            (Code::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
            (Code::DataLoss, StatusCode::INTERNAL_SERVER_ERROR),
            (Code::Unauthenticated, StatusCode::UNAUTHORIZED),
        ];

        for (code, expected_status) in test_cases {
            let err = ConnectError::from_code(code);
            assert_eq!(
                err.http_status_code(),
                expected_status,
                "Code::{:?} should map to {:?}",
                code,
                expected_status
            );
        }
    }

    // ---- StatusCode to Code conversion tests ----

    #[test]
    fn test_status_code_to_code_conversion() {
        assert!(matches!(Code::from(StatusCode::OK), Code::Ok));
        assert!(matches!(
            Code::from(StatusCode::BAD_REQUEST),
            Code::InvalidArgument
        ));
        assert!(matches!(
            Code::from(StatusCode::UNAUTHORIZED),
            Code::Unauthenticated
        ));
        assert!(matches!(
            Code::from(StatusCode::FORBIDDEN),
            Code::PermissionDenied
        ));
        assert!(matches!(Code::from(StatusCode::NOT_FOUND), Code::NotFound));
        assert!(matches!(
            Code::from(StatusCode::CONFLICT),
            Code::AlreadyExists
        ));
        assert!(matches!(
            Code::from(StatusCode::REQUEST_TIMEOUT),
            Code::DeadlineExceeded
        ));
        assert!(matches!(
            Code::from(StatusCode::TOO_MANY_REQUESTS),
            Code::ResourceExhausted
        ));
        assert!(matches!(
            Code::from(StatusCode::NOT_IMPLEMENTED),
            Code::Unimplemented
        ));
        assert!(matches!(
            Code::from(StatusCode::SERVICE_UNAVAILABLE),
            Code::Unavailable
        ));
        assert!(matches!(
            Code::from(StatusCode::INTERNAL_SERVER_ERROR),
            Code::Internal
        ));
        // Unknown status codes map to Code::Unknown
        assert!(matches!(Code::from(StatusCode::IM_A_TEAPOT), Code::Unknown));
    }

    #[test]
    fn test_from_status_code_and_string() {
        let err: ConnectError = (StatusCode::NOT_FOUND, "item not found".to_string()).into();
        assert!(matches!(err.code(), Code::NotFound));
        assert_eq!(err.message(), Some("item not found"));
    }

    // ---- Response generation tests ----

    #[test]
    fn test_into_response_unary_includes_metadata_as_headers() {
        let err = ConnectError::new(Code::Internal, "error")
            .with_meta("x-request-id", "req-123")
            .with_meta("x-trace-id", "trace-456");

        let response = err.into_response_with_protocol(RequestProtocol::ConnectUnaryJson);

        // Check status code
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Check metadata appears as headers
        assert_eq!(response.headers().get("x-request-id").unwrap(), "req-123");
        assert_eq!(response.headers().get("x-trace-id").unwrap(), "trace-456");

        // Check content-type
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_into_response_unary_proto_content_type() {
        let err = ConnectError::new(Code::NotFound, "not found");
        let response = err.into_response_with_protocol(RequestProtocol::ConnectUnaryProto);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json" // Errors are always JSON, even for proto requests
        );
    }

    #[test]
    fn test_into_response_streaming_returns_http_200() {
        let err = ConnectError::new(Code::Internal, "stream error");
        let response = err.into_response_with_protocol(RequestProtocol::ConnectStreamJson);

        // Streaming errors must return HTTP 200
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/connect+json"
        );
    }

    #[test]
    fn test_into_streaming_response_json() {
        let err = ConnectError::new(Code::Internal, "error");
        let response = err.into_streaming_response(false);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/connect+json"
        );
    }

    #[test]
    fn test_into_streaming_response_proto() {
        let err = ConnectError::new(Code::Internal, "error");
        let response = err.into_streaming_response(true);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/connect+proto"
        );
    }

    // ---- Serialization tests ----

    #[test]
    fn test_serialize_error_json() {
        let err = ConnectError::new(Code::InvalidArgument, "bad input");
        let json = serde_json::to_string(&err).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["code"], "invalid_argument");
        assert_eq!(parsed["message"], "bad input");
        // details should be absent when empty (skip_serializing_if)
        assert!(parsed.get("details").is_none());
    }

    #[test]
    fn test_serialize_error_with_details() {
        let err = ConnectError::new(Code::Internal, "error")
            .add_detail("google.rpc.RetryInfo", vec![1, 2, 3]);

        let json = serde_json::to_string(&err).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Details should be objects with type and value fields
        assert!(parsed.get("details").is_some());
        let details = parsed["details"].as_array().unwrap();
        assert_eq!(details.len(), 1);

        // Each detail should be {type, value} object
        let detail = &details[0];
        assert_eq!(detail["type"], "google.rpc.RetryInfo");
        assert_eq!(detail["value"], "AQID"); // base64 of [1, 2, 3] (no padding)
    }

    #[test]
    fn test_serialize_error_detail_strips_type_prefix() {
        // Type URLs with "type.googleapis.com/" prefix should have it stripped
        let err = ConnectError::new(Code::Internal, "error")
            .add_detail("type.googleapis.com/google.rpc.ErrorInfo", vec![1, 2]);

        let json = serde_json::to_string(&err).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let details = parsed["details"].as_array().unwrap();
        assert_eq!(details[0]["type"], "google.rpc.ErrorInfo"); // prefix stripped
    }

    #[test]
    fn test_serialize_error_without_message() {
        let err = ConnectError::from_code(Code::Unknown);
        let json = serde_json::to_string(&err).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["code"], "unknown");
        // message should be absent when None (skip_serializing_if)
        assert!(parsed.get("message").is_none());
    }

    // ---- Fallback response tests ----

    #[test]
    fn test_internal_error_response() {
        let response = internal_error_response("application/json");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_internal_error_end_stream_frame() {
        let frame = internal_error_end_stream_frame();

        // Check frame structure: flags (1 byte) + length (4 bytes) + payload
        assert!(frame.len() > 5);
        assert_eq!(frame[0], 0b0000_0010); // EndStream flag

        let length = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;
        assert_eq!(frame.len(), 5 + length);

        // Parse payload
        let payload = &frame[5..];
        let parsed: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert_eq!(parsed["error"]["code"], "internal");
    }

    #[test]
    fn test_internal_error_streaming_response() {
        let response = internal_error_streaming_response("application/connect+json");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/connect+json"
        );
    }

    // ---- Tonic conversion tests (feature-gated) ----

    #[cfg(feature = "tonic")]
    mod tonic_tests {
        use super::*;

        #[test]
        fn test_tonic_code_to_connect_code() {
            assert!(matches!(Code::from(::tonic::Code::Ok), Code::Ok));
            assert!(matches!(
                Code::from(::tonic::Code::Cancelled),
                Code::Canceled
            ));
            assert!(matches!(Code::from(::tonic::Code::Unknown), Code::Unknown));
            assert!(matches!(
                Code::from(::tonic::Code::InvalidArgument),
                Code::InvalidArgument
            ));
            assert!(matches!(
                Code::from(::tonic::Code::DeadlineExceeded),
                Code::DeadlineExceeded
            ));
            assert!(matches!(
                Code::from(::tonic::Code::NotFound),
                Code::NotFound
            ));
            assert!(matches!(
                Code::from(::tonic::Code::AlreadyExists),
                Code::AlreadyExists
            ));
            assert!(matches!(
                Code::from(::tonic::Code::PermissionDenied),
                Code::PermissionDenied
            ));
            assert!(matches!(
                Code::from(::tonic::Code::ResourceExhausted),
                Code::ResourceExhausted
            ));
            assert!(matches!(
                Code::from(::tonic::Code::FailedPrecondition),
                Code::FailedPrecondition
            ));
            assert!(matches!(Code::from(::tonic::Code::Aborted), Code::Aborted));
            assert!(matches!(
                Code::from(::tonic::Code::OutOfRange),
                Code::OutOfRange
            ));
            assert!(matches!(
                Code::from(::tonic::Code::Unimplemented),
                Code::Unimplemented
            ));
            assert!(matches!(
                Code::from(::tonic::Code::Internal),
                Code::Internal
            ));
            assert!(matches!(
                Code::from(::tonic::Code::Unavailable),
                Code::Unavailable
            ));
            assert!(matches!(
                Code::from(::tonic::Code::DataLoss),
                Code::DataLoss
            ));
            assert!(matches!(
                Code::from(::tonic::Code::Unauthenticated),
                Code::Unauthenticated
            ));
        }

        #[test]
        fn test_connect_code_to_tonic_code() {
            assert_eq!(::tonic::Code::from(Code::Ok), ::tonic::Code::Ok);
            assert_eq!(
                ::tonic::Code::from(Code::Canceled),
                ::tonic::Code::Cancelled
            );
            assert_eq!(::tonic::Code::from(Code::Unknown), ::tonic::Code::Unknown);
            assert_eq!(
                ::tonic::Code::from(Code::InvalidArgument),
                ::tonic::Code::InvalidArgument
            );
            assert_eq!(
                ::tonic::Code::from(Code::DeadlineExceeded),
                ::tonic::Code::DeadlineExceeded
            );
            assert_eq!(::tonic::Code::from(Code::NotFound), ::tonic::Code::NotFound);
            assert_eq!(
                ::tonic::Code::from(Code::AlreadyExists),
                ::tonic::Code::AlreadyExists
            );
            assert_eq!(
                ::tonic::Code::from(Code::PermissionDenied),
                ::tonic::Code::PermissionDenied
            );
            assert_eq!(
                ::tonic::Code::from(Code::ResourceExhausted),
                ::tonic::Code::ResourceExhausted
            );
            assert_eq!(
                ::tonic::Code::from(Code::FailedPrecondition),
                ::tonic::Code::FailedPrecondition
            );
            assert_eq!(::tonic::Code::from(Code::Aborted), ::tonic::Code::Aborted);
            assert_eq!(
                ::tonic::Code::from(Code::OutOfRange),
                ::tonic::Code::OutOfRange
            );
            assert_eq!(
                ::tonic::Code::from(Code::Unimplemented),
                ::tonic::Code::Unimplemented
            );
            assert_eq!(::tonic::Code::from(Code::Internal), ::tonic::Code::Internal);
            assert_eq!(
                ::tonic::Code::from(Code::Unavailable),
                ::tonic::Code::Unavailable
            );
            assert_eq!(::tonic::Code::from(Code::DataLoss), ::tonic::Code::DataLoss);
            assert_eq!(
                ::tonic::Code::from(Code::Unauthenticated),
                ::tonic::Code::Unauthenticated
            );
        }

        #[test]
        fn test_tonic_status_to_connect_error() {
            let status = ::tonic::Status::not_found("item not found");
            let err: ConnectError = status.into();

            assert!(matches!(err.code(), Code::NotFound));
            assert_eq!(err.message(), Some("item not found"));
        }

        #[test]
        fn test_tonic_status_empty_message() {
            let status = ::tonic::Status::new(::tonic::Code::Internal, "");
            let err: ConnectError = status.into();

            assert!(matches!(err.code(), Code::Internal));
            assert!(err.message().is_none());
        }

        #[test]
        fn test_connect_error_to_tonic_status() {
            let err = ConnectError::new(Code::PermissionDenied, "access denied");
            let status: ::tonic::Status = err.into();

            assert_eq!(status.code(), ::tonic::Code::PermissionDenied);
            assert_eq!(status.message(), "access denied");
        }

        #[test]
        fn test_connect_error_to_tonic_status_no_message() {
            let err = ConnectError::from_code(Code::Internal);
            let status: ::tonic::Status = err.into();

            assert_eq!(status.code(), ::tonic::Code::Internal);
            assert_eq!(status.message(), "");
        }
    }
}
