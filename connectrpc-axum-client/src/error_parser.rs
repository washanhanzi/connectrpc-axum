//! Error response parsing for Connect protocol.
//!
//! Parses JSON error responses from Connect servers into [`ClientError`].

use base64::Engine;
use connectrpc_axum_core::{Code, ErrorDetail};
use reqwest::Response;
use serde::Deserialize;

use crate::ClientError;

/// Parse an error response from the server.
///
/// Connect protocol error responses have the format:
/// ```json
/// {
///   "code": "not_found",
///   "message": "resource not found",
///   "details": [
///     {"type": "google.rpc.RetryInfo", "value": "base64-encoded-bytes"}
///   ]
/// }
/// ```
///
/// If the response body cannot be parsed as a Connect error, falls back to
/// creating an error based on the HTTP status code.
pub async fn parse_error_response(response: Response) -> ClientError {
    let status = response.status();

    // Try to read the response body
    let body_bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            return ClientError::Transport(format!(
                "failed to read error response body: {}",
                e
            ));
        }
    };

    // Try to parse as Connect error JSON
    match serde_json::from_slice::<ErrorResponseJson>(&body_bytes) {
        Ok(error_json) => {
            // Parse error code
            let code = error_json.code.parse().unwrap_or_else(|_| {
                // Fall back to deriving code from HTTP status
                http_status_to_code(status)
            });

            // Build ClientError
            let mut err = if let Some(message) = error_json.message {
                ClientError::new(code, message)
            } else {
                ClientError::from_code(code)
            };

            // Parse details
            for detail_json in error_json.details {
                if let Some(detail) = parse_error_detail(&detail_json) {
                    err = err.add_error_detail(detail);
                }
            }

            err
        }
        Err(_) => {
            // Couldn't parse as JSON, fall back to HTTP status code
            let code = http_status_to_code(status);
            let message = if body_bytes.is_empty() {
                status.canonical_reason().unwrap_or("Unknown error")
            } else {
                // Try to use body as message if it's valid UTF-8
                std::str::from_utf8(&body_bytes).unwrap_or("Unknown error")
            };
            ClientError::new(code, message)
        }
    }
}

/// JSON structure for Connect error responses.
#[derive(Deserialize)]
struct ErrorResponseJson {
    code: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    details: Vec<ErrorDetailJson>,
}

/// JSON structure for error details.
#[derive(Deserialize)]
struct ErrorDetailJson {
    #[serde(rename = "type")]
    type_url: String,
    #[serde(default)]
    value: String,
    // Some servers may include "debug" field which we ignore
}

/// Parse a single error detail from JSON.
fn parse_error_detail(json: &ErrorDetailJson) -> Option<ErrorDetail> {
    // Decode base64 value (Connect uses standard base64 without padding)
    let value = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(&json.value)
        .or_else(|_| {
            // Also try with padding in case server sends it
            base64::engine::general_purpose::STANDARD.decode(&json.value)
        })
        .ok()?;

    Some(ErrorDetail::new(&json.type_url, value))
}

/// Map HTTP status code to Connect error code.
///
/// This is used as a fallback when the response body doesn't contain
/// a valid Connect error JSON.
fn http_status_to_code(status: reqwest::StatusCode) -> Code {
    match status.as_u16() {
        200 => Code::Ok,
        400 => Code::InvalidArgument,
        401 => Code::Unauthenticated,
        403 => Code::PermissionDenied,
        404 => Code::NotFound,
        408 => Code::DeadlineExceeded,
        409 => Code::AlreadyExists,
        412 => Code::FailedPrecondition,
        416 => Code::OutOfRange,
        429 => Code::ResourceExhausted,
        499 => Code::Canceled, // Client Closed Request (nginx)
        500 => Code::Internal,
        501 => Code::Unimplemented,
        502..=504 => Code::Unavailable,
        _ => Code::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn test_http_status_to_code() {
        assert!(matches!(http_status_to_code(StatusCode::OK), Code::Ok));
        assert!(matches!(
            http_status_to_code(StatusCode::BAD_REQUEST),
            Code::InvalidArgument
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::UNAUTHORIZED),
            Code::Unauthenticated
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::FORBIDDEN),
            Code::PermissionDenied
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::NOT_FOUND),
            Code::NotFound
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::CONFLICT),
            Code::AlreadyExists
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::TOO_MANY_REQUESTS),
            Code::ResourceExhausted
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::INTERNAL_SERVER_ERROR),
            Code::Internal
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::NOT_IMPLEMENTED),
            Code::Unimplemented
        ));
        assert!(matches!(
            http_status_to_code(StatusCode::SERVICE_UNAVAILABLE),
            Code::Unavailable
        ));
    }

    #[test]
    fn test_parse_error_detail() {
        let json = ErrorDetailJson {
            type_url: "google.rpc.RetryInfo".to_string(),
            value: "AQID".to_string(), // base64 of [1, 2, 3] without padding
        };

        let detail = parse_error_detail(&json).unwrap();
        assert_eq!(detail.type_url(), "google.rpc.RetryInfo");
        assert_eq!(detail.value(), &[1, 2, 3]);
    }

    #[test]
    fn test_parse_error_detail_with_padding() {
        let json = ErrorDetailJson {
            type_url: "google.rpc.ErrorInfo".to_string(),
            value: "AQIDBA==".to_string(), // base64 of [1, 2, 3, 4] with padding
        };

        let detail = parse_error_detail(&json).unwrap();
        assert_eq!(detail.value(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_parse_error_detail_empty_value() {
        let json = ErrorDetailJson {
            type_url: "google.rpc.ErrorInfo".to_string(),
            value: "".to_string(),
        };

        let detail = parse_error_detail(&json).unwrap();
        assert_eq!(detail.value(), &[] as &[u8]);
    }

    #[test]
    fn test_parse_error_detail_invalid_base64() {
        let json = ErrorDetailJson {
            type_url: "google.rpc.ErrorInfo".to_string(),
            value: "not-valid-base64!!!".to_string(),
        };

        assert!(parse_error_detail(&json).is_none());
    }
}
