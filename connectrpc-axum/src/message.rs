//! Message types for Connect RPC request and response handling.
//!
//! This module provides request/response extraction, encoding, and decoding primitives.
//!
//! ## Request Primitives
//!
//! - [`read_body`]: Read HTTP body with size limit
//! - [`read_frame_bytes`]: Validate frame size against limits
//! - [`decompress_bytes`]: Decompress bytes based on encoding
//! - [`decode_proto`]: Decode protobuf message
//! - [`decode_json`]: Decode JSON message
//! - [`process_envelope_payload`]: Validate envelope flags and decompress payload
//!
//! ## Response Primitives
//!
//! - [`encode_proto`]: Encode protobuf message to bytes
//! - [`encode_json`]: Encode JSON message to bytes
//! - [`compress_bytes`]: Compress bytes if beneficial
//! - [`wrap_envelope`]: Wrap payload in a Connect streaming frame
//! - [`set_connect_content_encoding`]: Set Connect-Content-Encoding header

pub mod error;
pub mod request;
pub mod response;

pub use error::{build_end_stream_frame, Code, ConnectError, ErrorDetail, Metadata};
pub use request::{
    ConnectRequest, RequestPipeline, Streaming,
    // Primitive functions
    decode_json, decode_proto, decompress_bytes, envelope_flags,
    get_context_or_default, process_envelope_payload, read_body, read_frame_bytes,
};
pub use response::{
    ConnectResponse, ResponsePipeline, StreamBody,
    // Primitive functions
    compress_bytes, encode_json, encode_proto, set_connect_content_encoding, wrap_envelope,
};
