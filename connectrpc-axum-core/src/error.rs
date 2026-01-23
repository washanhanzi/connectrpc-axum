//! Connect protocol error codes and types.
//!
//! This module provides the core error types used by the Connect protocol:
//! - [`Code`]: Protocol status codes
//! - [`ErrorDetail`]: Self-describing error details
//! - [`EnvelopeError`]: Envelope framing errors

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
}
