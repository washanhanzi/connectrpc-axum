//! Response handling modules for Connect RPC client.
//!
//! This module contains response-side types and decoding:
//! - [`ConnectResponse`]: Response wrapper with metadata
//! - [`Metadata`]: HTTP headers wrapper
//! - [`Streaming`]: Streaming response wrapper
//! - [`FrameDecoder`]: Decodes Connect protocol envelope frames
//! - [`InterceptingStreaming`]: Streaming wrapper for message-level interception
//! - [`InterceptingSendStream`]: Stream wrapper for outgoing message interception

mod decoder;
pub(crate) mod error_parser;
mod intercepting;
mod streaming;
mod types;

pub use decoder::FrameDecoder;
#[allow(deprecated)]
pub use intercepting::InterceptingStream;
pub(crate) use intercepting::take_request_stream_error;
pub use intercepting::{
    InterceptingSendStream, InterceptingStreaming, RequestStreamError, TypedReceiveStreaming,
    TypedSendStream,
};
pub use streaming::Streaming;
pub use types::{ConnectResponse, Metadata};
pub(crate) use types::{ResponseMetadataMode, normalize_response_metadata};
