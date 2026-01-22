//! Connect protocol error codes and types.
//!
//! This module provides the core error types used by the Connect protocol:
//! - [`Code`]: Protocol status codes
//! - [`ErrorDetail`]: Self-describing error details
//! - [`ConnectError`]: Protocol error type

use serde::{Serialize, Serializer};

/// Connect RPC error codes, matching the codes defined in the Connect protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

impl Code {
    /// Get the string representation of this code.
    pub fn as_str(&self) -> &'static str {
        match self {
            Code::Ok => "ok",
            Code::Canceled => "canceled",
            Code::Unknown => "unknown",
            Code::InvalidArgument => "invalid_argument",
            Code::DeadlineExceeded => "deadline_exceeded",
            Code::NotFound => "not_found",
            Code::AlreadyExists => "already_exists",
            Code::PermissionDenied => "permission_denied",
            Code::ResourceExhausted => "resource_exhausted",
            Code::FailedPrecondition => "failed_precondition",
            Code::Aborted => "aborted",
            Code::OutOfRange => "out_of_range",
            Code::Unimplemented => "unimplemented",
            Code::Internal => "internal",
            Code::Unavailable => "unavailable",
            Code::DataLoss => "data_loss",
            Code::Unauthenticated => "unauthenticated",
        }
    }

    /// Returns whether this error code indicates a transient condition that may
    /// be resolved by retrying.
    ///
    /// The following codes are considered retryable:
    /// - [`Unavailable`](Code::Unavailable): Service is temporarily unavailable
    /// - [`ResourceExhausted`](Code::ResourceExhausted): Rate limited or quota exceeded
    /// - [`Aborted`](Code::Aborted): Transaction aborted, can be retried
    ///
    /// # Example
    ///
    /// ```
    /// use connectrpc_axum_core::Code;
    ///
    /// assert!(Code::Unavailable.is_retryable());
    /// assert!(Code::ResourceExhausted.is_retryable());
    /// assert!(!Code::NotFound.is_retryable());
    /// assert!(!Code::InvalidArgument.is_retryable());
    /// ```
    ///
    /// # Note
    ///
    /// For safe retries, the RPC should also be idempotent. Retrying a
    /// non-idempotent operation may cause unintended side effects.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Code::Unavailable | Code::ResourceExhausted | Code::Aborted
        )
    }

    /// Parse a code from its string representation.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ok" => Some(Code::Ok),
            "canceled" | "cancelled" => Some(Code::Canceled),
            "unknown" => Some(Code::Unknown),
            "invalid_argument" => Some(Code::InvalidArgument),
            "deadline_exceeded" => Some(Code::DeadlineExceeded),
            "not_found" => Some(Code::NotFound),
            "already_exists" => Some(Code::AlreadyExists),
            "permission_denied" => Some(Code::PermissionDenied),
            "resource_exhausted" => Some(Code::ResourceExhausted),
            "failed_precondition" => Some(Code::FailedPrecondition),
            "aborted" => Some(Code::Aborted),
            "out_of_range" => Some(Code::OutOfRange),
            "unimplemented" => Some(Code::Unimplemented),
            "internal" => Some(Code::Internal),
            "unavailable" => Some(Code::Unavailable),
            "data_loss" => Some(Code::DataLoss),
            "unauthenticated" => Some(Code::Unauthenticated),
            _ => None,
        }
    }
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

/// Connect protocol error variants.
///
/// This enum represents the different types of errors that can occur
/// during RPC communication.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ConnectError {
    /// A status error from the RPC handler with code, message, and optional details.
    #[error("{message:?}")]
    Status {
        code: Code,
        message: Option<String>,
        details: Vec<ErrorDetail>,
    },

    /// Transport-level error (connection failed, timeout, etc.).
    #[error("transport error: {0}")]
    Transport(String),

    /// Message encoding error.
    #[error("encode error: {0}")]
    Encode(String),

    /// Message decoding error.
    #[error("decode error: {0}")]
    Decode(String),

    /// Protocol error (malformed frames, unexpected data, etc.).
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl ConnectError {
    /// Create a new status error with a code and message.
    pub fn new<S: Into<String>>(code: Code, message: S) -> Self {
        ConnectError::Status {
            code,
            message: Some(message.into()),
            details: vec![],
        }
    }

    /// Create a new status error with just a code.
    pub fn from_code(code: Code) -> Self {
        ConnectError::Status {
            code,
            message: None,
            details: vec![],
        }
    }

    /// Get the error code.
    ///
    /// For non-Status variants, returns an appropriate code:
    /// - Transport: `Unavailable`
    /// - Encode/Decode: `Internal`
    /// - Protocol: `InvalidArgument`
    pub fn code(&self) -> Code {
        match self {
            ConnectError::Status { code, .. } => *code,
            ConnectError::Transport(_) => Code::Unavailable,
            ConnectError::Encode(_) | ConnectError::Decode(_) => Code::Internal,
            ConnectError::Protocol(_) => Code::InvalidArgument,
        }
    }

    /// Get the error message.
    pub fn message(&self) -> Option<&str> {
        match self {
            ConnectError::Status { message, .. } => message.as_deref(),
            ConnectError::Transport(msg)
            | ConnectError::Encode(msg)
            | ConnectError::Decode(msg)
            | ConnectError::Protocol(msg) => Some(msg),
        }
    }

    /// Get the error details (only for Status variant).
    pub fn details(&self) -> &[ErrorDetail] {
        match self {
            ConnectError::Status { details, .. } => details,
            _ => &[],
        }
    }

    /// Add an error detail with type URL and protobuf-encoded bytes.
    pub fn add_detail<S: Into<String>>(mut self, type_url: S, value: Vec<u8>) -> Self {
        if let ConnectError::Status { details, .. } = &mut self {
            details.push(ErrorDetail::new(type_url, value));
        }
        self
    }

    /// Add a pre-constructed ErrorDetail.
    pub fn add_error_detail(mut self, detail: ErrorDetail) -> Self {
        if let ConnectError::Status { details, .. } = &mut self {
            details.push(detail);
        }
        self
    }

    // Convenience constructors

    /// Create an unimplemented error.
    pub fn unimplemented<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unimplemented, message)
    }

    /// Create an invalid argument error.
    pub fn invalid_argument<S: Into<String>>(message: S) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    /// Create a not found error.
    pub fn not_found<S: Into<String>>(message: S) -> Self {
        Self::new(Code::NotFound, message)
    }

    /// Create a permission denied error.
    pub fn permission_denied<S: Into<String>>(message: S) -> Self {
        Self::new(Code::PermissionDenied, message)
    }

    /// Create an unauthenticated error.
    pub fn unauthenticated<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unauthenticated, message)
    }

    /// Create an internal error.
    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Internal, message)
    }

    /// Create an unavailable error.
    pub fn unavailable<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unavailable, message)
    }

    /// Create a resource exhausted error.
    pub fn resource_exhausted<S: Into<String>>(message: S) -> Self {
        Self::new(Code::ResourceExhausted, message)
    }

    /// Create a data loss error.
    pub fn data_loss<S: Into<String>>(message: S) -> Self {
        Self::new(Code::DataLoss, message)
    }

    /// Returns whether this error indicates a transient condition that may
    /// be resolved by retrying.
    ///
    /// This is a convenience wrapper for [`Code::is_retryable()`].
    ///
    /// # Example
    ///
    /// ```
    /// use connectrpc_axum_core::{Code, ConnectError};
    ///
    /// let err = ConnectError::unavailable("service overloaded");
    /// assert!(err.is_retryable());
    ///
    /// let err = ConnectError::not_found("resource missing");
    /// assert!(!err.is_retryable());
    ///
    /// // Transport errors are also retryable (they map to Unavailable)
    /// let err = ConnectError::Transport("connection reset".into());
    /// assert!(err.is_retryable());
    /// ```
    pub fn is_retryable(&self) -> bool {
        self.code().is_retryable()
    }
}

/// JSON body structure for error responses.
#[derive(Serialize)]
pub struct ErrorResponseBody {
    pub code: Code,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ErrorDetail>,
}

impl From<&ConnectError> for ErrorResponseBody {
    fn from(err: &ConnectError) -> Self {
        ErrorResponseBody {
            code: err.code(),
            message: err.message().map(String::from),
            details: err.details().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_as_str() {
        assert_eq!(Code::Ok.as_str(), "ok");
        assert_eq!(Code::InvalidArgument.as_str(), "invalid_argument");
        assert_eq!(Code::Unauthenticated.as_str(), "unauthenticated");
    }

    #[test]
    fn test_code_from_str() {
        assert_eq!(Code::from_str("ok"), Some(Code::Ok));
        assert_eq!(Code::from_str("invalid_argument"), Some(Code::InvalidArgument));
        assert_eq!(Code::from_str("canceled"), Some(Code::Canceled));
        assert_eq!(Code::from_str("cancelled"), Some(Code::Canceled)); // British spelling
        assert_eq!(Code::from_str("unknown_code"), None);
    }

    #[test]
    fn test_connect_error_new() {
        let err = ConnectError::new(Code::NotFound, "resource not found");
        assert_eq!(err.code(), Code::NotFound);
        assert_eq!(err.message(), Some("resource not found"));
        assert!(err.details().is_empty());
    }

    #[test]
    fn test_connect_error_from_code() {
        let err = ConnectError::from_code(Code::Internal);
        assert_eq!(err.code(), Code::Internal);
        assert!(err.message().is_none());
    }

    #[test]
    fn test_connect_error_variants_code() {
        let status = ConnectError::new(Code::NotFound, "not found");
        assert_eq!(status.code(), Code::NotFound);

        let transport = ConnectError::Transport("connection refused".into());
        assert_eq!(transport.code(), Code::Unavailable);

        let encode = ConnectError::Encode("serialization failed".into());
        assert_eq!(encode.code(), Code::Internal);

        let decode = ConnectError::Decode("deserialization failed".into());
        assert_eq!(decode.code(), Code::Internal);

        let protocol = ConnectError::Protocol("invalid frame".into());
        assert_eq!(protocol.code(), Code::InvalidArgument);
    }

    #[test]
    fn test_connect_error_add_detail() {
        let err = ConnectError::new(Code::Internal, "error")
            .add_detail("test.Type", vec![1, 2, 3]);

        assert_eq!(err.details().len(), 1);
        assert_eq!(err.details()[0].type_url(), "test.Type");
        assert_eq!(err.details()[0].value(), &[1, 2, 3]);
    }

    #[test]
    fn test_error_detail_serialize() {
        let detail = ErrorDetail::new("google.rpc.RetryInfo", vec![1, 2, 3]);
        let json = serde_json::to_string(&detail).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "google.rpc.RetryInfo");
        assert_eq!(parsed["value"], "AQID"); // base64 of [1, 2, 3] without padding
    }

    #[test]
    fn test_error_detail_serialize_strips_prefix() {
        let detail = ErrorDetail::new("type.googleapis.com/google.rpc.ErrorInfo", vec![1, 2]);
        let json = serde_json::to_string(&detail).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "google.rpc.ErrorInfo"); // prefix stripped
    }

    #[test]
    fn test_code_is_retryable() {
        // Retryable codes
        assert!(Code::Unavailable.is_retryable());
        assert!(Code::ResourceExhausted.is_retryable());
        assert!(Code::Aborted.is_retryable());

        // Non-retryable codes
        assert!(!Code::Ok.is_retryable());
        assert!(!Code::Canceled.is_retryable());
        assert!(!Code::Unknown.is_retryable());
        assert!(!Code::InvalidArgument.is_retryable());
        assert!(!Code::DeadlineExceeded.is_retryable());
        assert!(!Code::NotFound.is_retryable());
        assert!(!Code::AlreadyExists.is_retryable());
        assert!(!Code::PermissionDenied.is_retryable());
        assert!(!Code::FailedPrecondition.is_retryable());
        assert!(!Code::OutOfRange.is_retryable());
        assert!(!Code::Unimplemented.is_retryable());
        assert!(!Code::Internal.is_retryable());
        assert!(!Code::DataLoss.is_retryable());
        assert!(!Code::Unauthenticated.is_retryable());
    }

    #[test]
    fn test_connect_error_is_retryable() {
        // Status errors with retryable codes
        assert!(ConnectError::unavailable("service down").is_retryable());
        assert!(ConnectError::resource_exhausted("rate limited").is_retryable());
        assert!(ConnectError::new(Code::Aborted, "retry please").is_retryable());

        // Status errors with non-retryable codes
        assert!(!ConnectError::not_found("missing").is_retryable());
        assert!(!ConnectError::invalid_argument("bad input").is_retryable());
        assert!(!ConnectError::internal("server error").is_retryable());

        // Transport errors are retryable (map to Unavailable)
        assert!(ConnectError::Transport("connection reset".into()).is_retryable());

        // Encode/Decode/Protocol errors are not retryable
        assert!(!ConnectError::Encode("bad encoding".into()).is_retryable());
        assert!(!ConnectError::Decode("bad decoding".into()).is_retryable());
        assert!(!ConnectError::Protocol("bad frame".into()).is_retryable());
    }
}
