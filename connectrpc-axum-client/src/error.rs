//! Client-side Connect protocol error types.
//!
//! This module provides [`ClientError`], the error type for Connect RPC client operations.

use connectrpc_axum_core::{Code, EnvelopeError, ErrorDetail, Status};

/// Client-side Connect protocol error variants.
///
/// This enum represents the different types of errors that can occur
/// during client-side RPC communication.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ClientError {
    /// RPC status error from the server.
    ///
    /// This variant wraps the core [`Status`] type which contains
    /// the error code, message, and optional details.
    #[error("{0}")]
    Rpc(Status),

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

impl ClientError {
    /// Create a new status error with a code and message.
    pub fn new<S: Into<String>>(code: Code, message: S) -> Self {
        ClientError::Rpc(Status::new(code, message))
    }

    /// Create a new status error with just a code.
    pub fn from_code(code: Code) -> Self {
        ClientError::Rpc(Status::from_code(code))
    }

    /// Get the error code.
    ///
    /// For non-Rpc variants, returns an appropriate code:
    /// - Transport: `Unavailable`
    /// - Encode/Decode: `Internal`
    /// - Protocol: `InvalidArgument`
    pub fn code(&self) -> Code {
        match self {
            ClientError::Rpc(status) => status.code(),
            ClientError::Transport(_) => Code::Unavailable,
            ClientError::Encode(_) | ClientError::Decode(_) => Code::Internal,
            ClientError::Protocol(_) => Code::InvalidArgument,
        }
    }

    /// Get the error message.
    pub fn message(&self) -> Option<&str> {
        match self {
            ClientError::Rpc(status) => status.message(),
            ClientError::Transport(msg)
            | ClientError::Encode(msg)
            | ClientError::Decode(msg)
            | ClientError::Protocol(msg) => Some(msg),
        }
    }

    /// Get the error details (only for Rpc variant).
    pub fn details(&self) -> &[ErrorDetail] {
        match self {
            ClientError::Rpc(status) => status.details(),
            _ => &[],
        }
    }

    /// Add an error detail with type URL and protobuf-encoded bytes.
    pub fn add_detail<S: Into<String>>(self, type_url: S, value: Vec<u8>) -> Self {
        match self {
            ClientError::Rpc(status) => ClientError::Rpc(status.add_detail(type_url, value)),
            other => other,
        }
    }

    /// Add a pre-constructed ErrorDetail.
    pub fn add_error_detail(self, detail: ErrorDetail) -> Self {
        match self {
            ClientError::Rpc(status) => ClientError::Rpc(status.add_error_detail(detail)),
            other => other,
        }
    }

    /// Get the inner Status if this is an Rpc error.
    pub fn status(&self) -> Option<&Status> {
        match self {
            ClientError::Rpc(status) => Some(status),
            _ => None,
        }
    }

    /// Convert into the inner Status if this is an Rpc error.
    pub fn into_status(self) -> Option<Status> {
        match self {
            ClientError::Rpc(status) => Some(status),
            _ => None,
        }
    }

    // Convenience constructors

    /// Create an unimplemented error.
    pub fn unimplemented<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::unimplemented(message))
    }

    /// Create an invalid argument error.
    pub fn invalid_argument<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::invalid_argument(message))
    }

    /// Create a not found error.
    pub fn not_found<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::not_found(message))
    }

    /// Create a permission denied error.
    pub fn permission_denied<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::permission_denied(message))
    }

    /// Create an unauthenticated error.
    pub fn unauthenticated<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::unauthenticated(message))
    }

    /// Create an internal error.
    pub fn internal<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::internal(message))
    }

    /// Create an unavailable error.
    pub fn unavailable<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::unavailable(message))
    }

    /// Create a resource exhausted error.
    pub fn resource_exhausted<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::resource_exhausted(message))
    }

    /// Create a data loss error.
    pub fn data_loss<S: Into<String>>(message: S) -> Self {
        ClientError::Rpc(Status::data_loss(message))
    }

    /// Returns whether this error indicates a transient condition that may
    /// be resolved by retrying.
    ///
    /// This is a convenience wrapper for [`Code::is_retryable()`].
    ///
    /// # Example
    ///
    /// ```
    /// use connectrpc_axum_client::ClientError;
    ///
    /// let err = ClientError::unavailable("service overloaded");
    /// assert!(err.is_retryable());
    ///
    /// let err = ClientError::not_found("resource missing");
    /// assert!(!err.is_retryable());
    ///
    /// // Transport errors are also retryable (they map to Unavailable)
    /// let err = ClientError::Transport("connection reset".into());
    /// assert!(err.is_retryable());
    /// ```
    pub fn is_retryable(&self) -> bool {
        self.code().is_retryable()
    }
}

impl From<Status> for ClientError {
    fn from(status: Status) -> Self {
        ClientError::Rpc(status)
    }
}

impl From<EnvelopeError> for ClientError {
    fn from(err: EnvelopeError) -> Self {
        match err {
            EnvelopeError::IncompleteHeader { expected, actual } => {
                ClientError::Protocol(format!(
                    "incomplete envelope header: expected {} bytes, got {}",
                    expected, actual
                ))
            }
            EnvelopeError::InvalidFlags(flags) => {
                ClientError::Protocol(format!("invalid frame flags: 0x{:02x}", flags))
            }
            EnvelopeError::Decompression(msg) => {
                ClientError::Decode(format!("decompression failed: {}", msg))
            }
            EnvelopeError::Compression(msg) => {
                ClientError::Encode(format!("compression failed: {}", msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_error_new() {
        let err = ClientError::new(Code::NotFound, "resource not found");
        assert_eq!(err.code(), Code::NotFound);
        assert_eq!(err.message(), Some("resource not found"));
        assert!(err.details().is_empty());
    }

    #[test]
    fn test_client_error_from_code() {
        let err = ClientError::from_code(Code::Internal);
        assert_eq!(err.code(), Code::Internal);
        assert!(err.message().is_none());
    }

    #[test]
    fn test_client_error_variants_code() {
        let status = ClientError::new(Code::NotFound, "not found");
        assert_eq!(status.code(), Code::NotFound);

        let transport = ClientError::Transport("connection refused".into());
        assert_eq!(transport.code(), Code::Unavailable);

        let encode = ClientError::Encode("serialization failed".into());
        assert_eq!(encode.code(), Code::Internal);

        let decode = ClientError::Decode("deserialization failed".into());
        assert_eq!(decode.code(), Code::Internal);

        let protocol = ClientError::Protocol("invalid frame".into());
        assert_eq!(protocol.code(), Code::InvalidArgument);
    }

    #[test]
    fn test_client_error_add_detail() {
        let err =
            ClientError::new(Code::Internal, "error").add_detail("test.Type", vec![1, 2, 3]);

        assert_eq!(err.details().len(), 1);
        assert_eq!(err.details()[0].type_url(), "test.Type");
        assert_eq!(err.details()[0].value(), &[1, 2, 3]);
    }

    #[test]
    fn test_client_error_is_retryable() {
        // Status errors with retryable codes
        assert!(ClientError::unavailable("service down").is_retryable());
        assert!(ClientError::resource_exhausted("rate limited").is_retryable());
        assert!(ClientError::new(Code::Aborted, "retry please").is_retryable());

        // Status errors with non-retryable codes
        assert!(!ClientError::not_found("missing").is_retryable());
        assert!(!ClientError::invalid_argument("bad input").is_retryable());
        assert!(!ClientError::internal("server error").is_retryable());

        // Transport errors are retryable (map to Unavailable)
        assert!(ClientError::Transport("connection reset".into()).is_retryable());

        // Encode/Decode/Protocol errors are not retryable
        assert!(!ClientError::Encode("bad encoding".into()).is_retryable());
        assert!(!ClientError::Decode("bad decoding".into()).is_retryable());
        assert!(!ClientError::Protocol("bad frame".into()).is_retryable());
    }

    #[test]
    fn test_client_error_from_status() {
        let status = Status::not_found("resource missing");
        let err: ClientError = status.into();

        assert_eq!(err.code(), Code::NotFound);
        assert_eq!(err.message(), Some("resource missing"));
    }

    #[test]
    fn test_client_error_status_accessors() {
        let err = ClientError::new(Code::NotFound, "missing");
        assert!(err.status().is_some());
        assert_eq!(err.status().unwrap().code(), Code::NotFound);

        let err = ClientError::Transport("network error".into());
        assert!(err.status().is_none());

        let err = ClientError::new(Code::Internal, "error");
        let status = err.into_status();
        assert!(status.is_some());
        assert_eq!(status.unwrap().code(), Code::Internal);
    }

    #[test]
    fn test_client_error_rpc_variant_pattern_matching() {
        let err = ClientError::new(Code::NotFound, "user not found");

        // Users can pattern match on the Rpc variant
        match err {
            ClientError::Rpc(status) => {
                assert_eq!(status.code(), Code::NotFound);
                assert_eq!(status.message(), Some("user not found"));
            }
            _ => panic!("expected Rpc variant"),
        }
    }
}
