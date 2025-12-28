//! Protocol detection for incoming requests.

use crate::context::RequestProtocol;
use axum::http::{header, Method, Request};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
