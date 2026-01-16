//! Middleware layers for Connect RPC protocol handling.
//!
//! This module provides Tower layers for Connect protocol support:
//!
//! - [`ConnectLayer`]: Protocol detection, context building, timeouts, and message limits.
//! - [`BridgeLayer`]: Bridges Tower compression with Connect streaming requirements.
//!
//! ## Layer Stack Order
//!
//! When using Tower compression, layers should be ordered as:
//!
//! ```rust,ignore
//! use tower_http::compression::CompressionLayer;
//! use connectrpc_axum::{ConnectLayer, BridgeLayer};
//!
//! let app = Router::new()
//!     .route("/service/Method", post(handler))
//!     .layer(ConnectLayer::new())           // Inner: protocol handling
//!     .layer(CompressionLayer::new())       // Middle: HTTP body compression
//!     .layer(BridgeLayer::new())            // Outer: streaming validation
//! ```
//!
//! ## Without Compression
//!
//! If you don't need compression, you can skip the compression layers:
//!
//! ```rust,ignore
//! let app = Router::new()
//!     .route("/service/Method", post(handler))
//!     .layer(ConnectLayer::new());
//! ```

mod bridge;
mod connect;

pub use bridge::{BridgeLayer, BridgeService};
pub use connect::{ConnectLayer, ConnectService};
