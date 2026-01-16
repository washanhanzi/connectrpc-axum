//! Protocol detection and validation for Connect RPC.
//!
//! This module provides:
//! - [`RequestProtocol`] enum for identifying wire protocol variants
//! - [`detect_protocol`] for detecting protocol from incoming requests
//! - Validation functions for protocol version and content-type

use crate::error::{Code, ConnectError};
use axum::http::{Method, Request, header};

// ============================================================================
// Constants
// ============================================================================

/// The expected Connect protocol version.
pub const CONNECT_PROTOCOL_VERSION: &str = "1";

/// Header name for Connect protocol version.
pub const CONNECT_PROTOCOL_VERSION_HEADER: &str = "connect-protocol-version";

// ============================================================================
// RequestProtocol enum
// ============================================================================

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

// ============================================================================
// Protocol detection
// ============================================================================

/// Detect the protocol variant from an incoming request.
///
/// For GET requests, checks the `encoding` query parameter.
/// For POST requests, checks the `Content-Type` header.
pub fn detect_protocol<B>(req: &Request<B>) -> RequestProtocol {
    // GET requests: check query param for encoding
    if *req.method() == Method::GET {
        if let Some(query) = req.uri().query() {
            // Parse the encoding parameter
            // Query format: ?connect=v1&encoding=proto&message=...&base64=1
            for pair in query.split('&') {
                if let Some(value) = pair.strip_prefix("encoding=") {
                    return if value == "proto" {
                        RequestProtocol::ConnectUnaryProto
                    } else {
                        RequestProtocol::ConnectUnaryJson
                    };
                }
            }
        }
        // GET without encoding param defaults to JSON
        return RequestProtocol::ConnectUnaryJson;
    }

    // POST requests: check Content-Type header
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    RequestProtocol::from_content_type(content_type)
}

// ============================================================================
// Protocol version validation
// ============================================================================

/// Validate the Connect-Protocol-Version header for POST requests.
///
/// Returns `Some(ConnectError)` if validation fails, `None` if valid.
///
/// When `require_header` is false (default), the header is optional but if present must be "1".
/// When `require_header` is true, the header is required and must be "1".
pub fn validate_protocol_version<B>(
    req: &Request<B>,
    require_header: bool,
) -> Option<ConnectError> {
    let version = req
        .headers()
        .get(CONNECT_PROTOCOL_VERSION_HEADER)
        .and_then(|v| v.to_str().ok());

    match version {
        // Header present with correct value
        Some(v) if v == CONNECT_PROTOCOL_VERSION => None,
        // Header present with wrong value
        Some(v) => Some(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "connect-protocol-version must be \"{}\": got \"{}\"",
                CONNECT_PROTOCOL_VERSION, v
            ),
        )),
        // Header not present
        None if require_header => Some(ConnectError::new(
            Code::InvalidArgument,
            format!(
                "missing required header: set {} to \"{}\"",
                CONNECT_PROTOCOL_VERSION_HEADER, CONNECT_PROTOCOL_VERSION
            ),
        )),
        None => None,
    }
}

// ============================================================================
// Content-Type validation
// ============================================================================

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

// ============================================================================
// GET query parameter validation
// ============================================================================

/// Validate query parameters for GET unary requests.
///
/// Checks (matching connect-go behavior):
/// - `encoding` parameter is present and is "json" or "proto"
/// - `message` parameter is present
/// - `connect` parameter is "v1" if present, or required when `require_connect` is true
/// - `compression` parameter if present, is a supported algorithm ("gzip" or "identity")
///
/// Returns `Some(ConnectError)` if validation fails, `None` if valid.
pub fn validate_get_query_params<B>(
    req: &Request<B>,
    require_connect: bool,
) -> Option<ConnectError> {
    let query = req.uri().query().unwrap_or("");

    // Parse query parameters manually for efficiency
    let mut encoding: Option<&str> = None;
    let mut message_present = false;
    let mut connect: Option<&str> = None;
    let mut compression: Option<&str> = None;

    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "encoding" => encoding = Some(value),
                "message" => message_present = true,
                "connect" => connect = Some(value),
                "compression" => compression = Some(value),
                _ => {}
            }
        } else if pair == "message" {
            // Handle "message" without a value (empty message)
            message_present = true;
        }
    }

    // Validate connect=v1 parameter (matching connect-go: connectCheckProtocolVersion)
    match connect {
        None if require_connect => {
            return Some(ConnectError::new(
                Code::InvalidArgument,
                "missing required query parameter: set connect to \"v1\"",
            ));
        }
        Some(v) if !v.is_empty() && v != "v1" => {
            return Some(ConnectError::new(
                Code::InvalidArgument,
                format!("connect must be \"v1\": got \"{}\"", v),
            ));
        }
        _ => {}
    }

    // Validate encoding parameter is present
    let encoding = match encoding {
        None => {
            return Some(ConnectError::new(
                Code::InvalidArgument,
                "missing encoding parameter",
            ));
        }
        Some(v) => v,
    };

    // Validate encoding is a supported codec
    if encoding != "json" && encoding != "proto" {
        return Some(ConnectError::new(
            Code::InvalidArgument,
            format!("invalid message encoding: \"{}\"", encoding),
        ));
    }

    // Validate message parameter is present
    if !message_present {
        return Some(ConnectError::new(
            Code::InvalidArgument,
            "missing message parameter",
        ));
    }

    // Validate compression if present (matching connect-go: negotiateCompression)
    if let Some(comp) = compression
        && !comp.is_empty()
        && comp != "identity"
        && comp != "gzip"
    {
        return Some(ConnectError::new(
            Code::Unimplemented,
            format!(
                "unknown compression \"{}\": supported encodings are gzip, identity",
                comp
            ),
        ));
    }

    None
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- RequestProtocol tests ---

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

    // --- detect_protocol tests ---

    #[test]
    fn test_detect_protocol_post_json() {
        let req = Request::builder()
            .method(Method::POST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryJson);
    }

    #[test]
    fn test_detect_protocol_post_proto() {
        let req = Request::builder()
            .method(Method::POST)
            .header(header::CONTENT_TYPE, "application/proto")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryProto);
    }

    #[test]
    fn test_detect_protocol_post_connect_stream_json() {
        let req = Request::builder()
            .method(Method::POST)
            .header(header::CONTENT_TYPE, "application/connect+json")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectStreamJson);
    }

    #[test]
    fn test_detect_protocol_get_json() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=abc")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryJson);
    }

    #[test]
    fn test_detect_protocol_get_proto() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=proto&message=abc&base64=1")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryProto);
    }

    #[test]
    fn test_detect_protocol_get_no_encoding() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&message=abc")
            .body(())
            .unwrap();
        assert_eq!(detect_protocol(&req), RequestProtocol::ConnectUnaryJson);
    }

    // --- validate_protocol_version tests ---

    #[test]
    fn test_validate_protocol_version_valid() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, "1")
            .body(())
            .unwrap();
        assert!(validate_protocol_version(&req, false).is_none());
        assert!(validate_protocol_version(&req, true).is_none());
    }

    #[test]
    fn test_validate_protocol_version_missing_not_required() {
        let req = Request::builder().method(Method::POST).body(()).unwrap();
        assert!(validate_protocol_version(&req, false).is_none());
    }

    #[test]
    fn test_validate_protocol_version_missing_required() {
        let req = Request::builder().method(Method::POST).body(()).unwrap();
        let err = validate_protocol_version(&req, true);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err.message().unwrap().contains("missing required header"));
    }

    #[test]
    fn test_validate_protocol_version_wrong_version() {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONNECT_PROTOCOL_VERSION_HEADER, "2")
            .body(())
            .unwrap();
        let err = validate_protocol_version(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(
            err.message()
                .unwrap()
                .contains("connect-protocol-version must be")
        );
    }

    // --- validate_content_type tests ---

    #[test]
    fn test_validate_content_type_known() {
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
        assert!(
            err.message()
                .unwrap()
                .contains("streaming content-type not allowed for unary")
        );

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
        assert!(
            err.message()
                .unwrap()
                .contains("unary content-type not allowed for streaming")
        );

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

    // --- validate_get_query_params tests ---

    #[test]
    fn test_get_valid_request() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=%7B%7D")
            .body(())
            .unwrap();
        assert!(validate_get_query_params(&req, false).is_none());
        assert!(validate_get_query_params(&req, true).is_none());
    }

    #[test]
    fn test_get_valid_request_proto() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=proto&message=abc&base64=1")
            .body(())
            .unwrap();
        assert!(validate_get_query_params(&req, false).is_none());
    }

    #[test]
    fn test_get_missing_encoding() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&message=%7B%7D")
            .body(())
            .unwrap();
        let err = validate_get_query_params(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(
            err.message()
                .unwrap()
                .contains("missing encoding parameter")
        );
    }

    #[test]
    fn test_get_invalid_encoding() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=xml&message=%7B%7D")
            .body(())
            .unwrap();
        let err = validate_get_query_params(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err.message().unwrap().contains("invalid message encoding"));
    }

    #[test]
    fn test_get_missing_message() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json")
            .body(())
            .unwrap();
        let err = validate_get_query_params(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err.message().unwrap().contains("missing message parameter"));
    }

    #[test]
    fn test_get_missing_connect_not_required() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?encoding=json&message=%7B%7D")
            .body(())
            .unwrap();
        // When not required, missing connect is OK
        assert!(validate_get_query_params(&req, false).is_none());
    }

    #[test]
    fn test_get_missing_connect_required() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?encoding=json&message=%7B%7D")
            .body(())
            .unwrap();
        let err = validate_get_query_params(&req, true);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(
            err.message()
                .unwrap()
                .contains("missing required query parameter")
        );
    }

    #[test]
    fn test_get_invalid_connect_version() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v2&encoding=json&message=%7B%7D")
            .body(())
            .unwrap();
        let err = validate_get_query_params(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err.message().unwrap().contains("connect must be \"v1\""));
    }

    #[test]
    fn test_get_valid_compression_gzip() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=abc&compression=gzip")
            .body(())
            .unwrap();
        assert!(validate_get_query_params(&req, false).is_none());
    }

    #[test]
    fn test_get_valid_compression_identity() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=abc&compression=identity")
            .body(())
            .unwrap();
        assert!(validate_get_query_params(&req, false).is_none());
    }

    #[test]
    fn test_get_unsupported_compression() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=abc&compression=br")
            .body(())
            .unwrap();
        let err = validate_get_query_params(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::Unimplemented));
        assert!(err.message().unwrap().contains("unknown compression"));
    }

    #[test]
    fn test_get_empty_message_is_valid() {
        // Empty message value is valid (message= with no value)
        let req = Request::builder()
            .method(Method::GET)
            .uri("/svc/Method?connect=v1&encoding=json&message=")
            .body(())
            .unwrap();
        assert!(validate_get_query_params(&req, false).is_none());
    }
}
