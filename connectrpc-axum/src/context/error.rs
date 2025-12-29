//! Context error type - unified error handling for request/response processing.

use crate::context::RequestProtocol;
use crate::error::{Code, ConnectError};
use axum::response::Response;

/// Context processing error.
///
/// Handles both user-facing Connect errors and internal server errors.
/// Can optionally carry protocol information for response formatting.
#[derive(Debug)]
pub struct ContextError {
    kind: ContextErrorKind,
    /// Protocol for response formatting (set when building request context)
    protocol: Option<RequestProtocol>,
}

#[derive(Debug)]
enum ContextErrorKind {
    /// User-facing error - return directly to client
    Connect(ConnectError),
    /// Internal error - log, return generic internal error to client
    Internal(String),
}

impl ContextError {
    /// Create a Connect error (user-facing).
    pub fn connect(err: ConnectError) -> Self {
        Self {
            kind: ContextErrorKind::Connect(err),
            protocol: None,
        }
    }

    /// Create an internal error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            kind: ContextErrorKind::Internal(msg.into()),
            protocol: None,
        }
    }

    /// Set the protocol for response formatting.
    pub fn with_protocol(mut self, protocol: RequestProtocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// Convert to ConnectError for response.
    ///
    /// Internal errors become generic "internal error" message.
    /// The original error details are not exposed to clients for security.
    pub fn into_connect_error(self) -> ConnectError {
        match self.kind {
            ContextErrorKind::Connect(err) => err,
            ContextErrorKind::Internal(_msg) => {
                // Note: Internal error details not exposed to clients.
                // Callers should log the ContextError before conversion if needed.
                ConnectError::new(Code::Internal, "internal error")
            }
        }
    }

    /// Convert to HTTP response using the stored protocol.
    ///
    /// # Panics
    ///
    /// Panics if protocol was not set via `with_protocol()`.
    pub fn into_response(self) -> Response {
        let protocol = self
            .protocol
            .expect("ContextError::into_response requires protocol to be set");
        self.into_connect_error()
            .into_response_with_protocol(protocol)
    }
}

impl From<ConnectError> for ContextError {
    fn from(err: ConnectError) -> Self {
        Self::connect(err)
    }
}

impl From<std::io::Error> for ContextError {
    fn from(err: std::io::Error) -> Self {
        Self::internal(err.to_string())
    }
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ContextErrorKind::Connect(err) => {
                write!(f, "{}", err.message().unwrap_or("connect error"))
            }
            ContextErrorKind::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ContextError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_error_from_connect_error() {
        let err = ConnectError::new(Code::InvalidArgument, "test error");
        let ctx_err: ContextError = err.into();
        let connect_err = ctx_err.into_connect_error();
        assert!(matches!(connect_err.code(), Code::InvalidArgument));
    }

    #[test]
    fn test_context_error_internal() {
        let err = ContextError::internal("something went wrong");
        assert_eq!(format!("{err}"), "internal error: something went wrong");
    }

    #[test]
    fn test_context_error_into_connect_error() {
        // Connect error passes through
        let err = ContextError::connect(ConnectError::new(Code::NotFound, "not found"));
        let connect_err = err.into_connect_error();
        assert!(matches!(connect_err.code(), Code::NotFound));

        // Internal error becomes generic internal error
        let err = ContextError::internal("secret details");
        let connect_err = err.into_connect_error();
        assert!(matches!(connect_err.code(), Code::Internal));
        assert_eq!(connect_err.message(), Some("internal error"));
    }

    #[test]
    fn test_with_protocol() {
        let err = ContextError::connect(ConnectError::new(Code::NotFound, "not found"))
            .with_protocol(RequestProtocol::ConnectUnaryJson);
        // Should not panic
        let _response = err.into_response();
    }
}
