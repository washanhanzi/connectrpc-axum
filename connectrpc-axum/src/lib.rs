//! # Axum Connect
//!
//! A library for building [Connect](https://connect.build/) RPC services with [Axum](https://github.com/tokio-rs/axum).
//!
//! This crate provides a set of tools to build Connect-compliant RPC services that
//! feel idiomatic to Axum developers. It uses standard Axum extractors, response
//! types, and a compile-time route generator to integrate seamlessly into existing
//! Axum applications.
//!
//! ## Features
//!
//! - **Compile-time Route Generation:** `axum-connect-build` generates an `routes()` function from your `.proto` files, ensuring your routes are always in sync with your service definition.
//! - **Axum-native:** Handlers are standard `async fn` that use `axum::extract::FromRequest` and `axum::response::IntoResponse`.
//! - **Unary and Streaming:** Supports both unary and server-streaming RPCs.
//! - **Error Handling:** Provides a `ConnectError` type that automatically maps to
//!   Connect-compliant error responses.
//!
//! ## Getting Started
//!
//! Check out the `README.md` file for a comprehensive guide on how to get started.
//! The example in the `axum-connect-examples` directory is also a great resource.

pub mod error;
pub mod extractor;
pub mod handler;
pub mod response;
pub mod stream_response;

// Re-export several crates
pub use futures;
pub use pbjson;
pub use pbjson_types;
pub use prost;
pub use serde;

pub mod prelude {
    //! A prelude for `axum-connect` providing the most common types.
    pub use crate::error::{Code, ConnectError};
    pub use crate::extractor::ConnectRequest;
    pub use crate::handler::{ConnectHandler, ConnectService, connect_service};
    pub use crate::response::ConnectResponse;
    pub use crate::stream_response::ConnectStreamResponse;
}
