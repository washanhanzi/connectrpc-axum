//! Connect protocol version validation.
//!
//! This module validates the `Connect-Protocol-Version` header per the Connect spec.
//! The header is optional by default but can be required for stricter validation.

use crate::error::{Code, ConnectError};
use axum::http::Request;

/// The expected Connect protocol version.
pub const CONNECT_PROTOCOL_VERSION: &str = "1";

/// Header name for Connect protocol version.
pub const CONNECT_PROTOCOL_VERSION_HEADER: &str = "connect-protocol-version";

/// Validate the Connect-Protocol-Version header for POST requests.
///
/// Returns `Some(ConnectError)` if validation fails, `None` if valid.
///
/// When `require_header` is false (default), the header is optional but if present must be "1".
/// When `require_header` is true, the header is required and must be "1".
pub fn validate_protocol_version<B>(req: &Request<B>, require_header: bool) -> Option<ConnectError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request};

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
        // When require_header=false, missing header is OK
        let req = Request::builder().method(Method::POST).body(()).unwrap();
        assert!(validate_protocol_version(&req, false).is_none());
    }

    #[test]
    fn test_validate_protocol_version_missing_required() {
        // When require_header=true, missing header is an error
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
        // Wrong version is an error regardless of require_header setting
        let err = validate_protocol_version(&req, false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert!(matches!(err.code(), Code::InvalidArgument));
        assert!(err
            .message()
            .unwrap()
            .contains("connect-protocol-version must be"));
    }
}
