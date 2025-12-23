//! Protocol detection and per-request context for Connect RPC.
//!
//! This module provides the [`RequestProtocol`] enum for identifying the wire protocol
//! variant from incoming requests, and task-local storage for propagating this context
//! to response encoding.

use std::cell::Cell;

/// Protocol variant detected from the incoming request.
///
/// This determines how responses should be encoded:
/// - Content-Type header
/// - Whether envelope framing is needed
/// - Protobuf vs JSON encoding
///
/// Note: gRPC and gRPC-web are handled by `ContentTypeSwitch` which routes
/// them to Tonic, so there's no `GrpcProto` variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestProtocol {
    /// Connect unary with JSON encoding (`application/json`)
    /// Response: raw JSON, no frame envelope
    #[default]
    ConnectUnaryJson,

    /// Connect unary with protobuf encoding (`application/proto`)
    /// Response: raw protobuf bytes, no frame envelope
    ConnectUnaryProto,

    /// Connect streaming with JSON encoding (`application/connect+json`)
    /// Response: framed JSON messages with EndStream
    ConnectStreamJson,

    /// Connect streaming with protobuf encoding (`application/connect+proto`)
    /// Response: framed protobuf messages with EndStream
    ConnectStreamProto,
}

impl RequestProtocol {
    /// Detect protocol from Content-Type header value.
    ///
    /// Note: gRPC (`application/grpc*`) is handled by `ContentTypeSwitch` before
    /// reaching this code, so we don't need to detect it here.
    pub fn from_content_type(content_type: &str) -> Self {
        if content_type.starts_with("application/connect+proto") {
            Self::ConnectStreamProto
        } else if content_type.starts_with("application/connect+json") {
            Self::ConnectStreamJson
        } else if content_type.starts_with("application/proto") {
            Self::ConnectUnaryProto
        } else {
            // Default to JSON for application/json or unknown
            Self::ConnectUnaryJson
        }
    }

    /// Response Content-Type for successful responses.
    pub fn response_content_type(&self) -> &'static str {
        match self {
            Self::ConnectUnaryJson => "application/json",
            Self::ConnectUnaryProto => "application/proto",
            Self::ConnectStreamJson => "application/connect+json",
            Self::ConnectStreamProto => "application/connect+proto",
        }
    }

    /// Response Content-Type for error responses.
    ///
    /// For Connect unary, errors are always JSON regardless of request encoding.
    /// For streaming, errors use the same encoding as success responses.
    pub fn error_content_type(&self) -> &'static str {
        match self {
            // Connect unary errors are always JSON per spec
            Self::ConnectUnaryJson | Self::ConnectUnaryProto => "application/json",
            Self::ConnectStreamJson => "application/connect+json",
            Self::ConnectStreamProto => "application/connect+proto",
        }
    }

    /// Whether responses need envelope framing (5-byte header per message).
    ///
    /// - Connect unary: no framing (raw bytes)
    /// - Connect streaming: framing with EndStream message
    pub fn needs_envelope(&self) -> bool {
        matches!(self, Self::ConnectStreamJson | Self::ConnectStreamProto)
    }

    /// Whether to encode message bodies as protobuf (vs JSON).
    pub fn is_proto(&self) -> bool {
        matches!(self, Self::ConnectUnaryProto | Self::ConnectStreamProto)
    }

    /// Whether this is a streaming protocol variant.
    pub fn is_streaming(&self) -> bool {
        matches!(self, Self::ConnectStreamJson | Self::ConnectStreamProto)
    }
}

// Task-local storage for per-request protocol context.
// This replaces the previous thread-local storage which was unsound for async code.
tokio::task_local! {
    static REQUEST_PROTOCOL: Cell<RequestProtocol>;
}

/// Get the protocol for the current request.
///
/// Returns the protocol set by [`ConnectLayer`] middleware, or the default
/// (ConnectUnaryJson) if called outside a request context.
///
/// [`ConnectLayer`]: crate::layer::ConnectLayer
pub fn get_request_protocol() -> RequestProtocol {
    REQUEST_PROTOCOL
        .try_with(|p| p.get())
        .unwrap_or_default()
}

/// Scope for running code with a specific request protocol.
///
/// This is used internally by [`ConnectLayer`] to set the protocol for a request.
/// The protocol is available via [`get_request_protocol()`] within the scope.
///
/// [`ConnectLayer`]: crate::layer::ConnectLayer
pub async fn with_protocol<F, T>(protocol: RequestProtocol, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    REQUEST_PROTOCOL.scope(Cell::new(protocol), f).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_content_type() {
        assert_eq!(
            RequestProtocol::from_content_type("application/json"),
            RequestProtocol::ConnectUnaryJson
        );
        assert_eq!(
            RequestProtocol::from_content_type("application/json; charset=utf-8"),
            RequestProtocol::ConnectUnaryJson
        );
        assert_eq!(
            RequestProtocol::from_content_type("application/proto"),
            RequestProtocol::ConnectUnaryProto
        );
        assert_eq!(
            RequestProtocol::from_content_type("application/connect+json"),
            RequestProtocol::ConnectStreamJson
        );
        assert_eq!(
            RequestProtocol::from_content_type("application/connect+proto"),
            RequestProtocol::ConnectStreamProto
        );
        // Unknown defaults to JSON (gRPC is handled by ContentTypeSwitch)
        assert_eq!(
            RequestProtocol::from_content_type("text/plain"),
            RequestProtocol::ConnectUnaryJson
        );
    }

    #[test]
    fn test_response_content_type() {
        assert_eq!(
            RequestProtocol::ConnectUnaryJson.response_content_type(),
            "application/json"
        );
        assert_eq!(
            RequestProtocol::ConnectUnaryProto.response_content_type(),
            "application/proto"
        );
        assert_eq!(
            RequestProtocol::ConnectStreamJson.response_content_type(),
            "application/connect+json"
        );
        assert_eq!(
            RequestProtocol::ConnectStreamProto.response_content_type(),
            "application/connect+proto"
        );
    }

    #[test]
    fn test_error_content_type() {
        // Connect unary errors are always JSON
        assert_eq!(
            RequestProtocol::ConnectUnaryJson.error_content_type(),
            "application/json"
        );
        assert_eq!(
            RequestProtocol::ConnectUnaryProto.error_content_type(),
            "application/json"
        );
        // Streaming uses their normal content type
        assert_eq!(
            RequestProtocol::ConnectStreamJson.error_content_type(),
            "application/connect+json"
        );
        assert_eq!(
            RequestProtocol::ConnectStreamProto.error_content_type(),
            "application/connect+proto"
        );
    }

    #[test]
    fn test_needs_envelope() {
        assert!(!RequestProtocol::ConnectUnaryJson.needs_envelope());
        assert!(!RequestProtocol::ConnectUnaryProto.needs_envelope());
        assert!(RequestProtocol::ConnectStreamJson.needs_envelope());
        assert!(RequestProtocol::ConnectStreamProto.needs_envelope());
    }

    #[test]
    fn test_is_proto() {
        assert!(!RequestProtocol::ConnectUnaryJson.is_proto());
        assert!(RequestProtocol::ConnectUnaryProto.is_proto());
        assert!(!RequestProtocol::ConnectStreamJson.is_proto());
        assert!(RequestProtocol::ConnectStreamProto.is_proto());
    }
}
