//! Protocol detection for Connect RPC.
//!
//! This module provides the [`RequestProtocol`] enum for identifying the wire protocol
//! variant from incoming requests. The protocol is stored in request extensions by
//! [`ConnectLayer`] and injected into response types by handler wrappers.
//!
//! [`ConnectLayer`]: crate::layer::ConnectLayer

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

    /// Unknown or unsupported content-type.
    /// Requests with this protocol should be rejected.
    Unknown,
}

impl RequestProtocol {
    /// Detect protocol from Content-Type header value.
    ///
    /// Note: gRPC (`application/grpc*`) is handled by `ContentTypeSwitch` before
    /// reaching this code, so we don't need to detect it here.
    ///
    /// Returns `Unknown` for unrecognized content-types.
    pub fn from_content_type(content_type: &str) -> Self {
        if content_type.starts_with("application/connect+proto") {
            Self::ConnectStreamProto
        } else if content_type.starts_with("application/connect+json") {
            Self::ConnectStreamJson
        } else if content_type.starts_with("application/proto") {
            Self::ConnectUnaryProto
        } else if content_type.starts_with("application/json") {
            Self::ConnectUnaryJson
        } else {
            // Unknown content-type - should be rejected
            Self::Unknown
        }
    }

    /// Response Content-Type for successful responses.
    pub fn response_content_type(&self) -> &'static str {
        match self {
            Self::ConnectUnaryJson | Self::Unknown => "application/json",
            Self::ConnectUnaryProto => "application/proto",
            Self::ConnectStreamJson => "application/connect+json",
            Self::ConnectStreamProto => "application/connect+proto",
        }
    }

    /// Response Content-Type for error responses.
    ///
    /// For Connect unary, errors are always JSON regardless of request encoding.
    /// For streaming, errors use the same encoding as success responses.
    /// For unknown protocols, errors are JSON.
    pub fn error_content_type(&self) -> &'static str {
        match self {
            // Connect unary errors are always JSON per spec
            // Unknown protocol errors also use JSON
            Self::ConnectUnaryJson | Self::ConnectUnaryProto | Self::Unknown => "application/json",
            Self::ConnectStreamJson => "application/connect+json",
            Self::ConnectStreamProto => "application/connect+proto",
        }
    }

    /// Whether responses need envelope framing (5-byte header per message).
    ///
    /// - Connect unary: no framing (raw bytes)
    /// - Connect streaming: framing with EndStream message
    /// - Unknown: no framing (error responses are unary-style)
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

    /// Whether this is a unary protocol variant.
    pub fn is_unary(&self) -> bool {
        matches!(self, Self::ConnectUnaryJson | Self::ConnectUnaryProto)
    }

    /// Whether this protocol variant is valid (not Unknown).
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Get the streaming response content-type based on the encoding.
    ///
    /// For server-streaming endpoints where the request is unary but the
    /// response is streaming, use this to get the correct streaming content-type.
    pub fn streaming_response_content_type(&self) -> &'static str {
        if self.is_proto() {
            "application/connect+proto"
        } else {
            "application/connect+json"
        }
    }
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
    }

    #[test]
    fn test_from_content_type_unknown() {
        // Unknown content-types should return Unknown variant
        assert_eq!(
            RequestProtocol::from_content_type("text/plain"),
            RequestProtocol::Unknown
        );
        assert_eq!(
            RequestProtocol::from_content_type("application/xml"),
            RequestProtocol::Unknown
        );
        assert_eq!(
            RequestProtocol::from_content_type(""),
            RequestProtocol::Unknown
        );
        assert_eq!(
            RequestProtocol::from_content_type("invalid"),
            RequestProtocol::Unknown
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
        // Unknown uses JSON for errors
        assert_eq!(
            RequestProtocol::Unknown.error_content_type(),
            "application/json"
        );
    }

    #[test]
    fn test_needs_envelope() {
        assert!(!RequestProtocol::ConnectUnaryJson.needs_envelope());
        assert!(!RequestProtocol::ConnectUnaryProto.needs_envelope());
        assert!(RequestProtocol::ConnectStreamJson.needs_envelope());
        assert!(RequestProtocol::ConnectStreamProto.needs_envelope());
        assert!(!RequestProtocol::Unknown.needs_envelope());
    }

    #[test]
    fn test_is_proto() {
        assert!(!RequestProtocol::ConnectUnaryJson.is_proto());
        assert!(RequestProtocol::ConnectUnaryProto.is_proto());
        assert!(!RequestProtocol::ConnectStreamJson.is_proto());
        assert!(RequestProtocol::ConnectStreamProto.is_proto());
        assert!(!RequestProtocol::Unknown.is_proto());
    }

    #[test]
    fn test_is_streaming() {
        assert!(!RequestProtocol::ConnectUnaryJson.is_streaming());
        assert!(!RequestProtocol::ConnectUnaryProto.is_streaming());
        assert!(RequestProtocol::ConnectStreamJson.is_streaming());
        assert!(RequestProtocol::ConnectStreamProto.is_streaming());
        assert!(!RequestProtocol::Unknown.is_streaming());
    }

    #[test]
    fn test_is_unary() {
        assert!(RequestProtocol::ConnectUnaryJson.is_unary());
        assert!(RequestProtocol::ConnectUnaryProto.is_unary());
        assert!(!RequestProtocol::ConnectStreamJson.is_unary());
        assert!(!RequestProtocol::ConnectStreamProto.is_unary());
        assert!(!RequestProtocol::Unknown.is_unary());
    }

    #[test]
    fn test_is_valid() {
        assert!(RequestProtocol::ConnectUnaryJson.is_valid());
        assert!(RequestProtocol::ConnectUnaryProto.is_valid());
        assert!(RequestProtocol::ConnectStreamJson.is_valid());
        assert!(RequestProtocol::ConnectStreamProto.is_valid());
        assert!(!RequestProtocol::Unknown.is_valid());
    }
}
