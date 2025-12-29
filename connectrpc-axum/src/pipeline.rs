//! Request and response pipelines for Connect RPC.
//!
//! Pipelines handle the full request/response lifecycle:
//! - RequestPipeline: decode request (decompress, check limits, decode)
//! - ResponsePipeline: encode response (encode, compress)
//!
//! All configuration is read from Context in request extensions.

use crate::context::{
    compress, decompress, error::ContextError, CompressionEncoding, Context, RequestProtocol,
};
use crate::error::{Code, ConnectError};
use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use bytes::Bytes;
use prost::Message;
use serde::{de::DeserializeOwned, Serialize};

// ============================================================================
// RequestPipeline
// ============================================================================

/// Request pipeline - decodes incoming request messages.
///
/// Handles: body reading, decompression, size limits, protocol decoding.
pub struct RequestPipeline;

impl RequestPipeline {
    /// Decode request message from HTTP request.
    ///
    /// Reads Context from extensions, reads body, decompresses, decodes.
    pub async fn decode<T>(req: Request<Body>) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        // 1. Get context from extensions (injected by layer)
        let ctx = req
            .extensions()
            .get::<Context>()
            .copied()
            .ok_or_else(|| ContextError::internal("missing request context"))?;

        // 2. Read body bytes with size limit
        let max_size = ctx.limits.max_message_size().unwrap_or(usize::MAX);
        let body = axum::body::to_bytes(req.into_body(), max_size)
            .await
            .map_err(|e| {
                ContextError::connect(ConnectError::new(
                    Code::ResourceExhausted,
                    format!("failed to read request body: {e}"),
                ))
            })?;

        // 3. Decode using the body
        Self::decode_bytes(&ctx, body)
    }

    /// Decode from raw bytes (for use when body is already read).
    pub fn decode_bytes<T>(ctx: &Context, body: Bytes) -> Result<T, ContextError>
    where
        T: Message + DeserializeOwned + Default,
    {
        // 1. Decompress if needed
        let body = if ctx.compression.request_encoding != CompressionEncoding::Identity {
            let decompressed =
                decompress(&body, ctx.compression.request_encoding).map_err(|e| {
                    ContextError::connect(ConnectError::new(
                        Code::InvalidArgument,
                        format!("decompression failed: {e}"),
                    ))
                })?;
            Bytes::from(decompressed)
        } else {
            body
        };

        // 2. Check decompressed size
        ctx.limits.check_size(body.len()).map_err(|msg| {
            ContextError::connect(ConnectError::new(Code::ResourceExhausted, msg))
        })?;

        // 3. Decode based on protocol
        decode_message(&body, ctx.protocol)
    }
}

// ============================================================================
// ResponsePipeline
// ============================================================================

/// Response pipeline - encodes outgoing response messages.
///
/// Handles: protocol encoding, compression, HTTP response building.
pub struct ResponsePipeline;

impl ResponsePipeline {
    /// Encode response message to HTTP response.
    ///
    /// Reads Context from request extensions.
    pub fn encode<T>(req: &Request<Body>, message: &T) -> Result<Response<Body>, ContextError>
    where
        T: Message + Serialize,
    {
        let ctx = req
            .extensions()
            .get::<Context>()
            .ok_or_else(|| ContextError::internal("missing request context"))?;

        Self::encode_with_context(ctx, message)
    }

    /// Encode with explicit context (when request not available).
    pub fn encode_with_context<T>(
        ctx: &Context,
        message: &T,
    ) -> Result<Response<Body>, ContextError>
    where
        T: Message + Serialize,
    {
        // 1. Encode based on protocol
        let body = encode_message(message, ctx.protocol)?;

        // 2. Compress if beneficial
        let compression = &ctx.compression;
        let (body, content_encoding) =
            if compression.response_encoding != CompressionEncoding::Identity
                && body.len() >= compression.min_compress_bytes
            {
                let compressed = compress(&body, compression.response_encoding)?;
                (compressed, Some(compression.response_encoding.as_str()))
            } else {
                (body, None)
            };

        // 3. Build HTTP response
        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctx.protocol.response_content_type());

        if let Some(encoding) = content_encoding {
            builder = builder.header(header::CONTENT_ENCODING, encoding);
        }

        builder
            .body(Body::from(body))
            .map_err(|e| ContextError::internal(e.to_string()))
    }
}

// ============================================================================
// Encoding/Decoding helpers
// ============================================================================

/// Decode a message from bytes based on protocol.
fn decode_message<T>(body: &[u8], protocol: RequestProtocol) -> Result<T, ContextError>
where
    T: Message + DeserializeOwned + Default,
{
    if protocol.is_proto() {
        T::decode(body).map_err(|e| {
            ContextError::connect(ConnectError::new(
                Code::InvalidArgument,
                format!("failed to decode protobuf message: {e}"),
            ))
        })
    } else {
        serde_json::from_slice(body).map_err(|e| {
            ContextError::connect(ConnectError::new(
                Code::InvalidArgument,
                format!("failed to decode JSON message: {e}"),
            ))
        })
    }
}

/// Encode a message to bytes based on protocol.
fn encode_message<T>(message: &T, protocol: RequestProtocol) -> Result<Vec<u8>, ContextError>
where
    T: Message + Serialize,
{
    if protocol.is_proto() {
        Ok(message.encode_to_vec())
    } else {
        serde_json::to_vec(message).map_err(|e| ContextError::internal(e.to_string()))
    }
}
