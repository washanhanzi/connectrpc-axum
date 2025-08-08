//! Error and response types for Connect.
//!
//! This module provides error types that conform to the Connect RPC protocol,
//! including support for error codes, messages, details, and metadata.

use axum::{
    Json,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Serialize, Serializer};

/// Connect RPC error codes, matching the codes defined in the Connect protocol.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    Ok = 0,
    Canceled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

/// An error that captures the key pieces of information for Connect RPC:
/// a code, an optional message, metadata (HTTP headers), and optional error details.
#[derive(Clone, Debug)]
pub struct ConnectError {
    code: Code,
    message: Option<String>,
    details: Vec<Vec<u8>>, // Raw bytes of error details
    meta: HeaderMap,
}

impl ConnectError {
    /// Create a new error with a code and message.
    pub fn new<S: Into<String>>(code: Code, message: S) -> Self {
        Self {
            code,
            message: Some(message.into()),
            details: vec![],
            meta: HeaderMap::new(),
        }
    }

    /// Create a new error with just a code.
    pub fn from_code(code: Code) -> Self {
        Self {
            code,
            message: None,
            details: vec![],
            meta: HeaderMap::new(),
        }
    }

    /// Create an unimplemented error.
    pub fn new_unimplemented() -> Self {
        Self {
            code: Code::Unimplemented,
            message: Some("The requested service has not been implemented.".to_string()),
            details: vec![],
            meta: HeaderMap::new(),
        }
    }

    /// Get the error code.
    pub fn code(&self) -> Code {
        self.code
    }

    /// Get the error message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Get the error details as raw bytes.
    pub fn details(&self) -> &[Vec<u8>] {
        &self.details
    }

    /// Add an error detail (raw bytes).
    pub fn add_detail(mut self, detail: Vec<u8>) -> Self {
        self.details.push(detail);
        self
    }

    /// Get the metadata headers.
    pub fn meta(&self) -> &HeaderMap {
        &self.meta
    }

    /// Get mutable access to metadata headers.
    pub fn meta_mut(&mut self) -> &mut HeaderMap {
        &mut self.meta
    }

    /// Add a metadata header.
    pub fn with_meta<K, V>(mut self, key: K, value: V) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        if let Ok(name) = HeaderName::from_bytes(key.as_ref().as_bytes()) {
            if let Ok(val) = HeaderValue::from_str(value.as_ref()) {
                self.meta.append(name, val);
            }
        }
        self
    }

    /// Set metadata from HeaderMap.
    pub fn set_meta_from_headers(mut self, headers: &HeaderMap) -> Self {
        self.meta = headers.clone();
        self
    }
}

impl IntoResponse for ConnectError {
    fn into_response(self) -> Response {
        let status_code = match self.code {
            Code::Ok => StatusCode::OK,
            Code::Canceled => StatusCode::REQUEST_TIMEOUT,
            Code::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
            Code::InvalidArgument => StatusCode::BAD_REQUEST,
            Code::DeadlineExceeded => StatusCode::REQUEST_TIMEOUT,
            Code::NotFound => StatusCode::NOT_FOUND,
            Code::AlreadyExists => StatusCode::CONFLICT,
            Code::PermissionDenied => StatusCode::FORBIDDEN,
            Code::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
            Code::FailedPrecondition => StatusCode::BAD_REQUEST, // Connect spec says this should be 400
            Code::Aborted => StatusCode::CONFLICT,
            Code::OutOfRange => StatusCode::BAD_REQUEST,
            Code::Unimplemented => StatusCode::NOT_IMPLEMENTED,
            Code::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Code::DataLoss => StatusCode::INTERNAL_SERVER_ERROR,
            Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        };

        // Create the error response body
        let error_body = ErrorResponseBody {
            code: self.code,
            message: self.message,
            details: self.details,
        };

        // Start with the base response
        let mut response = (status_code, Json(error_body)).into_response();

        // Add metadata as headers
        let headers = response.headers_mut();
        headers.extend(self.meta.iter().map(|(k, v)| (k.clone(), v.clone())));

        response
    }
}

/// The JSON body structure for error responses.
#[derive(Serialize)]
struct ErrorResponseBody {
    code: Code,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_details"
    )]
    details: Vec<Vec<u8>>,
}

/// Serialize details as base64-encoded strings
fn serialize_details<S>(details: &[Vec<u8>], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use base64::Engine;
    use serde::ser::SerializeSeq;

    let mut seq = serializer.serialize_seq(Some(details.len()))?;
    for detail in details {
        let encoded = base64::engine::general_purpose::STANDARD.encode(detail);
        seq.serialize_element(&encoded)?;
    }
    seq.end()
}

impl Serialize for ConnectError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize only the parts that should go in the JSON body
        ErrorResponseBody {
            code: self.code,
            message: self.message.clone(),
            details: self.details.clone(),
        }
        .serialize(serializer)
    }
}
