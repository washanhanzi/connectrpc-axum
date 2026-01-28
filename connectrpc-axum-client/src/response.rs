//! Response handling modules for Connect RPC client.
//!
//! This module contains response-side types and decoding:
//! - [`ConnectResponse`]: Response wrapper with metadata
//! - [`Metadata`]: HTTP headers wrapper
//! - [`Streaming`]: Streaming response wrapper
//! - [`FrameDecoder`]: Decodes Connect protocol envelope frames
//! - [`InterceptingStream`]: Stream wrapper for message-level interception
//! - [`InterceptingSendStream`]: Stream wrapper for outgoing message interception

mod decoder;
pub(crate) mod error_parser;
mod intercepting;
mod streaming;
mod types;

pub use decoder::FrameDecoder;
pub use intercepting::{InterceptingSendStream, InterceptingStream, InterceptingStreaming};
pub use streaming::Streaming;
pub use types::{ConnectResponse, Metadata};
