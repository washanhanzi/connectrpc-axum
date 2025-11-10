//! Extractor for Connect requests.
use crate::error::Code;
use crate::error::ConnectError;
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{Method, header},
};
use prost::Message;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::cell::Cell;

const APPLICATION_PROTO: &str = "application/proto";
const APPLICATION_CONNECT_PROTO: &str = "application/connect+proto";
const APPLICATION_JSON: &str = "application/json";
const APPLICATION_CONNECT_JSON: &str = "application/connect+json";

/// Content format used in the request, stored in thread-local storage
/// so the response can match the format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentFormat {
    Json,
    Proto,
}

thread_local! {
    static REQUEST_FORMAT: Cell<ContentFormat> = const { Cell::new(ContentFormat::Json) };
}

/// Store the request content format for this request
pub fn set_request_format(format: ContentFormat) {
    REQUEST_FORMAT.with(|f| f.set(format));
}

/// Get the request content format for this request
pub fn get_request_format() -> ContentFormat {
    REQUEST_FORMAT.with(|f| f.get())
}

#[derive(Debug, Clone)]
pub struct ConnectRequest<T>(pub T);

impl<S, T> FromRequest<S> for ConnectRequest<T>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    type Rejection = ConnectError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match *req.method() {
            Method::POST => from_post_request(req, state).await,
            Method::GET => from_get_request(req, state).await,
            _ => Err(ConnectError::new(
                Code::Unimplemented,
                "HTTP method not supported".to_string(),
            )),
        }
    }
}

async fn from_post_request<S, T>(
    req: Request,
    _state: &S,
) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let content_type = content_type.to_string();

    println!("📥 Incoming request Content-Type: {}", content_type);

    // Determine the format and store it in thread-local storage
    let format = if content_type.starts_with(APPLICATION_PROTO)
        || content_type.starts_with(APPLICATION_CONNECT_PROTO)
        || content_type.starts_with("application/grpc") {
        ContentFormat::Proto
    } else {
        ContentFormat::Json
    };
    set_request_format(format);

    let mut bytes = Bytes::from_request(req, _state)
        .await
        .map_err(|err| ConnectError::new(Code::Internal, err.to_string()))?;

    println!("📦 Raw bytes received: length={}, first_bytes={:?}",
        bytes.len(),
        &bytes[..bytes.len().min(10)]);

    // For Connect streaming content types AND gRPC, unwrap the 5-byte frame envelope
    // Both protocols use the same frame format: [flags:1][length:4][payload:length]
    let is_connect_streaming = content_type.starts_with(APPLICATION_CONNECT_PROTO)
        || content_type.starts_with(APPLICATION_CONNECT_JSON);
    let is_grpc = content_type.starts_with("application/grpc");
    let needs_frame_unwrap = is_connect_streaming || is_grpc;

    if needs_frame_unwrap && bytes.len() >= 5 {
        println!("📦 Unwrapping frame envelope (protocol: {})",
            if is_grpc { "gRPC" } else { "Connect" });
        // Frame format: [flags:1][length:4][payload:length]
        let flags = bytes[0];
        let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
        println!("  Flags: 0x{:02X}, Length: {}", flags, length);

        // Validate compression flag for gRPC
        if is_grpc && flags > 1 {
            return Err(ConnectError::new(
                Code::InvalidArgument,
                format!("invalid gRPC compression flag: {} (valid flags are 0 and 1)", flags),
            ));
        }

        // Extract the actual payload
        if bytes.len() >= 5 + length {
            bytes = bytes.slice(5..5 + length);
            println!("  Unwrapped payload length: {}", bytes.len());
        } else {
            return Err(ConnectError::new(
                Code::InvalidArgument,
                format!("incomplete frame: expected {} bytes, got {}", 5 + length, bytes.len()),
            ));
        }
    } else if needs_frame_unwrap {
        println!("⚠️  Content-Type indicates framing but bytes.len()={} < 5, skipping frame unwrap", bytes.len());
    } else {
        println!("📦 Not a framed request, using raw bytes");
    }

    if content_type.starts_with(APPLICATION_PROTO)
        || content_type.starts_with(APPLICATION_CONNECT_PROTO)
        || content_type.starts_with("application/grpc") {
        println!("📦 Decoding protobuf, bytes length: {}", bytes.len());
        let message = T::decode(bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;
        println!("✅ Protobuf decode successful");
        Ok(ConnectRequest(message))
    } else if content_type.starts_with(APPLICATION_JSON) || content_type.starts_with(APPLICATION_CONNECT_JSON) {
        println!("📦 Decoding JSON, bytes length: {}", bytes.len());
        let message: T = serde_json::from_slice(&bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;
        println!("✅ JSON decode successful");
        Ok(ConnectRequest(message))
    } else {
        Err(ConnectError::new(
            Code::InvalidArgument,
            format!("unsupported content-type: {}", content_type),
        ))
    }
}

#[derive(Deserialize)]
struct GetRequestQuery {
    connect: String,
    encoding: String,
    message: String,
    base64: Option<String>,
    #[allow(dead_code)] // Not used yet, but part of the spec
    compression: Option<String>,
}

async fn from_get_request<S, T>(req: Request, _state: &S) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    let query = req.uri().query().unwrap_or("");
    let params: GetRequestQuery = serde_qs::from_str(query)
        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;

    if params.connect != "v1" {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported connect version",
        ));
    }

    let bytes = if params.base64.as_deref() == Some("true") {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::URL_SAFE
            .decode(&params.message)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        params.message.into_bytes()
    };

    let message = if params.encoding == "proto" {
        T::decode(bytes.as_slice())
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else if params.encoding == "json" {
        serde_json::from_slice(&bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported encoding",
        ));
    };

    Ok(ConnectRequest(message))
}
