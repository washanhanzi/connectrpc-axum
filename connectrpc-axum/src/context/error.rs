//! Response error type - bundles ConnectError with protocol for response formatting.

use crate::context::RequestProtocol;
use crate::error::{Code, ConnectError};
use axum::response::Response;

/// Error bundled with protocol for HTTP response formatting.
///
/// Used internally by the framework to carry protocol information
/// alongside errors for proper JSON/Proto encoding in responses.
#[derive(Debug)]
pub struct ContextError(pub RequestProtocol, pub ConnectError);

impl ContextError {
    /// Create a new response error.
    pub fn new(protocol: RequestProtocol, err: ConnectError) -> Self {
        Self(protocol, err)
    }

    /// Create an internal error (hides details from client).
    ///
    /// The provided message is not exposed to clients for security.
    /// Clients receive a generic "internal error" message.
    pub fn internal(protocol: RequestProtocol, _msg: impl Into<String>) -> Self {
        // Note: _msg could be logged here if needed
        Self(
            protocol,
            ConnectError::new(Code::Internal, "internal error"),
        )
    }

    /// Get the protocol.
    pub fn protocol(&self) -> RequestProtocol {
        self.0
    }

    /// Get the underlying ConnectError.
    pub fn error(&self) -> &ConnectError {
        &self.1
    }

    /// Convert to HTTP response with proper encoding.
    pub fn into_response(self) -> Response {
        self.1.into_response_with_protocol(self.0)
    }

    /// Extract the underlying ConnectError.
    pub fn into_connect_error(self) -> ConnectError {
        self.1
    }
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.1.message().unwrap_or("error"))
    }
}

impl std::error::Error for ContextError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_error_new() {
        let err = ContextError::new(
            RequestProtocol::ConnectUnaryJson,
            ConnectError::new(Code::InvalidArgument, "test error"),
        );
        assert!(matches!(err.error().code(), Code::InvalidArgument));
        assert!(matches!(err.protocol(), RequestProtocol::ConnectUnaryJson));
    }

    #[test]
    fn test_response_error_internal() {
        let err = ContextError::internal(RequestProtocol::ConnectUnaryJson, "secret details");
        // Internal errors hide the real message
        assert!(matches!(err.error().code(), Code::Internal));
        assert_eq!(err.error().message(), Some("internal error"));
    }

    #[test]
    fn test_into_response() {
        let err = ContextError::new(
            RequestProtocol::ConnectUnaryJson,
            ConnectError::new(Code::NotFound, "not found"),
        );
        let _response = err.into_response();
    }

    #[test]
    fn test_display() {
        let err = ContextError::new(
            RequestProtocol::ConnectUnaryProto,
            ConnectError::new(Code::NotFound, "not found"),
        );
        assert_eq!(format!("{err}"), "not found");
    }
}
