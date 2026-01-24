//! Request encoding modules for Connect RPC client.
//!
//! This module contains request-side encoding:
//! - [`FrameEncoder`]: Encodes messages into Connect protocol envelope frames

mod encoder;

pub use encoder::FrameEncoder;
