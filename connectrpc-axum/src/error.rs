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

/// An error that captures the key pieces of information for Connect RPC:
/// a code, an optional message, metadata (HTTP headers), and optional error details.
#[derive(Clone, Debug)]
pub struct ConnectError {
    code: Code,
    message: Option<String>,
    details: Vec<Vec<u8>>, // Raw bytes of error details
    meta: HeaderMap,
}

impl ConnectError {
    /// Create a new error with a code and message.
    pub fn new<S: Into<String>>(code: Code, message: S) -> Self {
        Self {
            code,
            message: Some(message.into()),
            details: vec![],
            meta: HeaderMap::new(),
        }
    }

    /// Create a new error with just a code.
    pub fn from_code(code: Code) -> Self {
        Self {
            code,
            message: None,
            details: vec![],
            meta: HeaderMap::new(),
        }
    }

    /// Create an unimplemented error.
    pub fn new_unimplemented() -> Self {
        Self {
            code: Code::Unimplemented,
            message: Some("The requested service has not been implemented.".to_string()),
            details: vec![],
            meta: HeaderMap::new(),
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

    /// Get the error details as raw bytes.
    pub fn details(&self) -> &[Vec<u8>] {
        &self.details
    }

    /// Add an error detail (raw bytes).
    pub fn add_detail(mut self, detail: Vec<u8>) -> Self {
        self.details.push(detail);
        self
    }

    /// Get the metadata headers.
    pub fn meta(&self) -> &HeaderMap {
        &self.meta
    }

    /// Get mutable access to metadata headers.
    pub fn meta_mut(&mut self) -> &mut HeaderMap {
        &mut self.meta
    }

    /// Add a metadata header.
    pub fn with_meta<K, V>(mut self, key: K, value: V) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        if let Ok(name) = HeaderName::from_bytes(key.as_ref().as_bytes()) {
            if let Ok(val) = HeaderValue::from_str(value.as_ref()) {
                self.meta.append(name, val);
            }
        }
        self
    }

    /// Set metadata from HeaderMap.
    pub fn set_meta_from_headers(mut self, headers: &HeaderMap) -> Self {
        self.meta = headers.clone();
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
        let headers = response.headers_mut();
        headers.extend(self.meta.iter().map(|(k, v)| (k.clone(), v.clone())));

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

// ---- Conversions from HTTP status codes ----

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
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_details"
    )]
    details: Vec<Vec<u8>>,
}

/// Serialize details as base64-encoded strings
fn serialize_details<S>(details: &[Vec<u8>], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use base64::Engine;
    use serde::ser::SerializeSeq;

    let mut seq = serializer.serialize_seq(Some(details.len()))?;
    for detail in details {
        let encoded = base64::engine::general_purpose::STANDARD.encode(detail);
        seq.serialize_element(&encoded)?;
    }
    seq.end()
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
