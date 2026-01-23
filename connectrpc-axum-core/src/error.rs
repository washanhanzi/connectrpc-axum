//! Connect protocol error codes and types.
//!
//! This module provides the core error types used by the Connect protocol:
//! - [`Code`]: Protocol status codes
//! - [`ErrorDetail`]: Self-describing error details
//! - [`EnvelopeError`]: Envelope framing errors

use std::str::FromStr;

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
}

/// Error returned when parsing a [`Code`] from a string fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseCodeError(());

impl std::fmt::Display for ParseCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown error code")
    }
}

impl std::error::Error for ParseCodeError {}

impl FromStr for Code {
    type Err = ParseCodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(Code::Ok),
            "canceled" | "cancelled" => Ok(Code::Canceled),
            "unknown" => Ok(Code::Unknown),
            "invalid_argument" => Ok(Code::InvalidArgument),
            "deadline_exceeded" => Ok(Code::DeadlineExceeded),
            "not_found" => Ok(Code::NotFound),
            "already_exists" => Ok(Code::AlreadyExists),
            "permission_denied" => Ok(Code::PermissionDenied),
            "resource_exhausted" => Ok(Code::ResourceExhausted),
            "failed_precondition" => Ok(Code::FailedPrecondition),
            "aborted" => Ok(Code::Aborted),
            "out_of_range" => Ok(Code::OutOfRange),
            "unimplemented" => Ok(Code::Unimplemented),
            "internal" => Ok(Code::Internal),
            "unavailable" => Ok(Code::Unavailable),
            "data_loss" => Ok(Code::DataLoss),
            "unauthenticated" => Ok(Code::Unauthenticated),
            _ => Err(ParseCodeError(())),
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

/// Envelope framing errors.
///
/// This error type is used for errors that occur during envelope parsing
/// and decompression in the Connect streaming protocol.
#[derive(Clone, Debug, thiserror::Error)]
pub enum EnvelopeError {
    /// Incomplete envelope header.
    #[error("incomplete envelope header: expected {expected} bytes, got {actual}")]
    IncompleteHeader { expected: usize, actual: usize },

    /// Invalid frame flags.
    #[error("invalid frame flags: 0x{0:02x}")]
    InvalidFlags(u8),

    /// Decompression failed.
    #[error("decompression failed: {0}")]
    Decompression(String),

    /// Compression failed.
    #[error("compression failed: {0}")]
    Compression(String),
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

// ============================================================================
// Status - Core RPC error type shared between client and server
// ============================================================================

/// RPC status representing the result of an RPC call.
///
/// This is the core error data type shared between client and server.
/// Contains the error code, optional message, and optional structured details.
///
/// # Example
///
/// ```
/// use connectrpc_axum_core::Status;
///
/// // Create a status error
/// let status = Status::not_found("user not found");
/// assert_eq!(status.code().as_str(), "not_found");
/// assert_eq!(status.message(), Some("user not found"));
///
/// // Add error details
/// let status = status.add_detail("google.rpc.RetryInfo", vec![1, 2, 3]);
/// assert_eq!(status.details().len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct Status {
    code: Code,
    message: Option<String>,
    details: Vec<ErrorDetail>,
}

impl Status {
    /// Create a new status with a code and message.
    pub fn new<S: Into<String>>(code: Code, message: S) -> Self {
        Self {
            code,
            message: Some(message.into()),
            details: vec![],
        }
    }

    /// Create a new status with just a code.
    pub fn from_code(code: Code) -> Self {
        Self {
            code,
            message: None,
            details: vec![],
        }
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

    /// Returns whether this error indicates a transient condition that may
    /// be resolved by retrying.
    ///
    /// This is a convenience wrapper for [`Code::is_retryable()`].
    pub fn is_retryable(&self) -> bool {
        self.code.is_retryable()
    }

    // Convenience constructors for all error codes

    /// Create a canceled status.
    pub fn cancelled<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Canceled, message)
    }

    /// Create an unknown status.
    pub fn unknown<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unknown, message)
    }

    /// Create an invalid argument status.
    pub fn invalid_argument<S: Into<String>>(message: S) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    /// Create a deadline exceeded status.
    pub fn deadline_exceeded<S: Into<String>>(message: S) -> Self {
        Self::new(Code::DeadlineExceeded, message)
    }

    /// Create a not found status.
    pub fn not_found<S: Into<String>>(message: S) -> Self {
        Self::new(Code::NotFound, message)
    }

    /// Create an already exists status.
    pub fn already_exists<S: Into<String>>(message: S) -> Self {
        Self::new(Code::AlreadyExists, message)
    }

    /// Create a permission denied status.
    pub fn permission_denied<S: Into<String>>(message: S) -> Self {
        Self::new(Code::PermissionDenied, message)
    }

    /// Create a resource exhausted status.
    pub fn resource_exhausted<S: Into<String>>(message: S) -> Self {
        Self::new(Code::ResourceExhausted, message)
    }

    /// Create a failed precondition status.
    pub fn failed_precondition<S: Into<String>>(message: S) -> Self {
        Self::new(Code::FailedPrecondition, message)
    }

    /// Create an aborted status.
    pub fn aborted<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Aborted, message)
    }

    /// Create an out of range status.
    pub fn out_of_range<S: Into<String>>(message: S) -> Self {
        Self::new(Code::OutOfRange, message)
    }

    /// Create an unimplemented status.
    pub fn unimplemented<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unimplemented, message)
    }

    /// Create an internal status.
    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Internal, message)
    }

    /// Create an unavailable status.
    pub fn unavailable<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unavailable, message)
    }

    /// Create a data loss status.
    pub fn data_loss<S: Into<String>>(message: S) -> Self {
        Self::new(Code::DataLoss, message)
    }

    /// Create an unauthenticated status.
    pub fn unauthenticated<S: Into<String>>(message: S) -> Self {
        Self::new(Code::Unauthenticated, message)
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code.as_str())?;
        if let Some(msg) = &self.message {
            write!(f, ": {}", msg)?;
        }
        Ok(())
    }
}

impl std::error::Error for Status {}

impl Serialize for Status {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ErrorResponseBody {
            code: self.code,
            message: self.message.clone(),
            details: self.details.clone(),
        }
        .serialize(serializer)
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
        assert_eq!("ok".parse(), Ok(Code::Ok));
        assert_eq!("invalid_argument".parse(), Ok(Code::InvalidArgument));
        assert_eq!("canceled".parse(), Ok(Code::Canceled));
        assert_eq!("cancelled".parse(), Ok(Code::Canceled)); // British spelling
        assert_eq!("unknown_code".parse::<Code>(), Err(ParseCodeError(())));
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
    fn test_envelope_error_display() {
        let err = EnvelopeError::IncompleteHeader {
            expected: 5,
            actual: 3,
        };
        assert_eq!(
            err.to_string(),
            "incomplete envelope header: expected 5 bytes, got 3"
        );

        let err = EnvelopeError::InvalidFlags(0xFF);
        assert_eq!(err.to_string(), "invalid frame flags: 0xff");

        let err = EnvelopeError::Decompression("gzip failed".into());
        assert_eq!(err.to_string(), "decompression failed: gzip failed");

        let err = EnvelopeError::Compression("gzip failed".into());
        assert_eq!(err.to_string(), "compression failed: gzip failed");
    }

    // ========================================================================
    // Status tests
    // ========================================================================

    #[test]
    fn test_status_new() {
        let status = Status::new(Code::NotFound, "resource not found");
        assert_eq!(status.code(), Code::NotFound);
        assert_eq!(status.message(), Some("resource not found"));
        assert!(status.details().is_empty());
    }

    #[test]
    fn test_status_from_code() {
        let status = Status::from_code(Code::Internal);
        assert_eq!(status.code(), Code::Internal);
        assert!(status.message().is_none());
    }

    #[test]
    fn test_status_convenience_constructors() {
        assert_eq!(Status::cancelled("msg").code(), Code::Canceled);
        assert_eq!(Status::unknown("msg").code(), Code::Unknown);
        assert_eq!(Status::invalid_argument("msg").code(), Code::InvalidArgument);
        assert_eq!(Status::deadline_exceeded("msg").code(), Code::DeadlineExceeded);
        assert_eq!(Status::not_found("msg").code(), Code::NotFound);
        assert_eq!(Status::already_exists("msg").code(), Code::AlreadyExists);
        assert_eq!(Status::permission_denied("msg").code(), Code::PermissionDenied);
        assert_eq!(Status::resource_exhausted("msg").code(), Code::ResourceExhausted);
        assert_eq!(Status::failed_precondition("msg").code(), Code::FailedPrecondition);
        assert_eq!(Status::aborted("msg").code(), Code::Aborted);
        assert_eq!(Status::out_of_range("msg").code(), Code::OutOfRange);
        assert_eq!(Status::unimplemented("msg").code(), Code::Unimplemented);
        assert_eq!(Status::internal("msg").code(), Code::Internal);
        assert_eq!(Status::unavailable("msg").code(), Code::Unavailable);
        assert_eq!(Status::data_loss("msg").code(), Code::DataLoss);
        assert_eq!(Status::unauthenticated("msg").code(), Code::Unauthenticated);
    }

    #[test]
    fn test_status_add_detail() {
        let status = Status::new(Code::Internal, "error")
            .add_detail("test.Type1", vec![1, 2, 3])
            .add_detail("test.Type2", vec![4, 5, 6]);

        assert_eq!(status.details().len(), 2);
        assert_eq!(status.details()[0].type_url(), "test.Type1");
        assert_eq!(status.details()[0].value(), &[1, 2, 3]);
    }

    #[test]
    fn test_status_is_retryable() {
        assert!(Status::unavailable("service down").is_retryable());
        assert!(Status::resource_exhausted("rate limited").is_retryable());
        assert!(Status::aborted("retry please").is_retryable());

        assert!(!Status::not_found("missing").is_retryable());
        assert!(!Status::invalid_argument("bad input").is_retryable());
        assert!(!Status::internal("server error").is_retryable());
    }

    #[test]
    fn test_status_display() {
        let status = Status::not_found("resource missing");
        assert_eq!(status.to_string(), "not_found: resource missing");

        let status = Status::from_code(Code::Internal);
        assert_eq!(status.to_string(), "internal");
    }

    #[test]
    fn test_status_serialize() {
        let status = Status::new(Code::NotFound, "not found")
            .add_detail("google.rpc.RetryInfo", vec![1, 2, 3]);

        let json = serde_json::to_string(&status).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["code"], "not_found");
        assert_eq!(parsed["message"], "not found");
        assert!(parsed["details"].is_array());
        assert_eq!(parsed["details"][0]["type"], "google.rpc.RetryInfo");
    }
}
