//! Common types for Connect RPC request handling.
//!
//! This module provides types used by both the [`ConnectLayer`] middleware
//! and request extensions, including protocol detection, timeout configuration,
//! and message size limits.
//!
//! [`ConnectLayer`]: crate::layer::ConnectLayer

pub mod limit;
pub mod protocol;
pub mod timeout;

pub use limit::{DEFAULT_MAX_MESSAGE_SIZE, MessageLimits};
pub use protocol::RequestProtocol;
pub use timeout::ConnectTimeout;
