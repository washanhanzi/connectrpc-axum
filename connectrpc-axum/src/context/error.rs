//! Response error type - bundles ConnectError with protocol for response formatting.

use crate::context::RequestProtocol;
use crate::context::protocol::SUPPORTED_CONTENT_TYPES;
use crate::message::error::{Code, ConnectError};
use axum::body::Body;
use axum::http::StatusCode;
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

// ============================================================================
// Protocol Negotiation Error
// ============================================================================

/// Pre-protocol error for unsupported media types.
///
/// This error type produces raw HTTP 415 responses that bypass Connect error
/// formatting. Per connect-go behavior, unsupported content-types and invalid
/// GET encodings return HTTP 415 Unsupported Media Type with an `Accept-Post`
/// header listing supported content types.
///
/// This is used before protocol detection completes, when the request cannot
/// be handled by any supported protocol variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolNegotiationError {
    /// Content-Type or encoding is not supported.
    UnsupportedMediaType,
}

impl ProtocolNegotiationError {
    /// Convert to HTTP 415 response with Accept-Post header.
    pub fn into_response(self) -> Response {
        Response::builder()
            .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .header("Accept-Post", SUPPORTED_CONTENT_TYPES)
            .body(Body::empty())
            .unwrap()
    }
}

impl std::fmt::Display for ProtocolNegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedMediaType => write!(f, "unsupported media type"),
        }
    }
}

impl std::error::Error for ProtocolNegotiationError {}

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

    // --- ProtocolNegotiationError tests ---

    #[test]
    fn test_protocol_negotiation_error_into_response() {
        use axum::http::StatusCode;

        let err = ProtocolNegotiationError::UnsupportedMediaType;
        let response = err.into_response();

        // Should return HTTP 415
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // Should have Accept-Post header with supported content types
        let accept_post = response.headers().get("Accept-Post");
        assert!(accept_post.is_some());
        let accept_post_value = accept_post.unwrap().to_str().unwrap();
        assert!(accept_post_value.contains("application/json"));
        assert!(accept_post_value.contains("application/proto"));
        assert!(accept_post_value.contains("application/connect+json"));
        assert!(accept_post_value.contains("application/connect+proto"));
    }

    #[test]
    fn test_protocol_negotiation_error_display() {
        let err = ProtocolNegotiationError::UnsupportedMediaType;
        assert_eq!(format!("{err}"), "unsupported media type");
    }

    #[test]
    fn test_protocol_negotiation_error_debug() {
        let err = ProtocolNegotiationError::UnsupportedMediaType;
        assert_eq!(format!("{err:?}"), "UnsupportedMediaType");
    }
}
