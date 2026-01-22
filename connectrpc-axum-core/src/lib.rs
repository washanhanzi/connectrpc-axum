//! Core protocol types for ConnectRPC.
//!
//! This crate provides shared types and functions used by both the server
//! (`connectrpc-axum`) and client (`connectrpc-axum-client`) crates.
//!
//! ## Modules
//!
//! - [`error`]: Protocol error codes and error types
//! - [`codec`]: Compression codec trait and implementations
//! - [`compression`]: Compression configuration types
//! - [`envelope`]: Streaming envelope framing functions

mod codec;
mod compression;
mod envelope;
mod error;

pub use codec::*;
pub use compression::*;
pub use envelope::*;
pub use error::*;
