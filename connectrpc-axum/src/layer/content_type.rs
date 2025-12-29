//! Content-Type validation for Connect RPC.
//!
//! This module validates that incoming requests use a known Connect protocol
//! Content-Type, matching connect-go behavior.
//!
//! ## Validation Levels
//!
//! - **Layer level**: Rejects unknown content-types before reaching handlers
//! - **Handler level**: Rejects mismatched content-types (unary vs streaming)

use crate::context::RequestProtocol;
use crate::error::{Code, ConnectError};

/// Validate that the detected protocol is a known Connect content-type.
///
/// Returns `Some(ConnectError)` if the content-type is unknown/unsupported.
/// Returns `None` if the content-type is valid.
///
/// Per Connect protocol spec (matching connect-go), unknown content-types
/// are rejected with `Code::Unknown` (HTTP 500).
pub fn validate_content_type(protocol: RequestProtocol) -> Option<ConnectError> {
    if protocol.is_valid() {
        None
    } else {
        Some(ConnectError::new(Code::Unknown, "unsupported content-type"))
    }
}

/// Validate that the protocol is appropriate for unary RPC.
///
/// Returns `Some(ConnectError)` if a streaming content-type is used for a unary RPC.
/// Returns `None` if the content-type is valid for unary.
///
/// Unary RPCs accept: `application/json`, `application/proto`
/// Unary RPCs reject: `application/connect+json`, `application/connect+proto`
pub fn validate_unary_content_type(protocol: RequestProtocol) -> Option<ConnectError> {
    if protocol.is_unary() {
        None
    } else if protocol.is_streaming() {
        Some(ConnectError::new(
            Code::Unknown,
            "streaming content-type not allowed for unary RPC",
        ))
    } else {
        // Unknown protocol - already handled by validate_content_type
        Some(ConnectError::new(Code::Unknown, "unsupported content-type"))
    }
}

/// Validate that the protocol is appropriate for streaming RPC.
///
/// Returns `Some(ConnectError)` if a unary content-type is used for a streaming RPC.
/// Returns `None` if the content-type is valid for streaming.
///
/// Streaming RPCs accept: `application/connect+json`, `application/connect+proto`
/// Streaming RPCs reject: `application/json`, `application/proto`
pub fn validate_streaming_content_type(protocol: RequestProtocol) -> Option<ConnectError> {
    if protocol.is_streaming() {
        None
    } else if protocol.is_unary() {
        Some(ConnectError::new(
            Code::Unknown,
            "unary content-type not allowed for streaming RPC",
        ))
    } else {
        // Unknown protocol
        Some(ConnectError::new(Code::Unknown, "unsupported content-type"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_content_type_known() {
        // All valid content-types should pass
        assert!(validate_content_type(RequestProtocol::ConnectUnaryJson).is_none());
        assert!(validate_content_type(RequestProtocol::ConnectUnaryProto).is_none());
        assert!(validate_content_type(RequestProtocol::ConnectStreamJson).is_none());
        assert!(validate_content_type(RequestProtocol::ConnectStreamProto).is_none());
    }

    #[test]
    fn test_validate_content_type_unknown() {
        let err = validate_content_type(RequestProtocol::Unknown);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::Unknown));
        assert!(err.message().unwrap().contains("unsupported content-type"));
    }

    #[test]
    fn test_validate_unary_accepts_unary() {
        assert!(validate_unary_content_type(RequestProtocol::ConnectUnaryJson).is_none());
        assert!(validate_unary_content_type(RequestProtocol::ConnectUnaryProto).is_none());
    }

    #[test]
    fn test_validate_unary_rejects_streaming() {
        let err = validate_unary_content_type(RequestProtocol::ConnectStreamJson);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::Unknown));
        assert!(err
            .message()
            .unwrap()
            .contains("streaming content-type not allowed for unary"));

        let err = validate_unary_content_type(RequestProtocol::ConnectStreamProto);
        assert!(err.is_some());
        assert!(matches!(err.unwrap().code(), Code::Unknown));
    }

    #[test]
    fn test_validate_unary_rejects_unknown() {
        let err = validate_unary_content_type(RequestProtocol::Unknown);
        assert!(err.is_some());
        assert!(matches!(err.unwrap().code(), Code::Unknown));
    }

    #[test]
    fn test_validate_streaming_accepts_streaming() {
        assert!(validate_streaming_content_type(RequestProtocol::ConnectStreamJson).is_none());
        assert!(validate_streaming_content_type(RequestProtocol::ConnectStreamProto).is_none());
    }

    #[test]
    fn test_validate_streaming_rejects_unary() {
        let err = validate_streaming_content_type(RequestProtocol::ConnectUnaryJson);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::Unknown));
        assert!(err
            .message()
            .unwrap()
            .contains("unary content-type not allowed for streaming"));

        let err = validate_streaming_content_type(RequestProtocol::ConnectUnaryProto);
        assert!(err.is_some());
        assert!(matches!(err.unwrap().code(), Code::Unknown));
    }

    #[test]
    fn test_validate_streaming_rejects_unknown() {
        let err = validate_streaming_content_type(RequestProtocol::Unknown);
        assert!(err.is_some());
        assert!(matches!(err.unwrap().code(), Code::Unknown));
    }
}
