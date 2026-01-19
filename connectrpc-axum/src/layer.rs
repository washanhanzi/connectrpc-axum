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
//!
//! ## Why BridgeLayer Exists
//!
//! The Connect protocol uses two different compression mechanisms:
//!
//! - **Unary RPCs**: Use standard HTTP `Content-Encoding`/`Accept-Encoding` headers.
//!   Tower's `CompressionLayer` handles this automatically.
//! - **Streaming RPCs**: Use `Connect-Content-Encoding`/`Connect-Accept-Encoding` headers.
//!   Each message envelope is individually compressed. HTTP body compression must be disabled.
//!
//! The [`BridgeLayer`] ensures streaming requests don't get double-compressed by setting
//! `Accept-Encoding: identity` for streaming requests (prevents Tower from compressing response).
//! It also enforces request body size limits (on compressed size) before decompression,
//! protecting against oversized payloads.
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              BridgeLayer                    │  ← Size limit check, streaming headers
//! │  ┌───────────────────────────────────────┐  │
//! │  │     Tower CompressionLayer            │  │  ← HTTP body compression (unary only)
//! │  │  ┌─────────────────────────────────┐  │  │
//! │  │  │         ConnectLayer            │  │  │  ← Protocol detection, context
//! │  │  │  ┌───────────────────────────┐  │  │  │
//! │  │  │  │          Handler          │  │  │  │  ← Your RPC handlers
//! │  │  │  └───────────────────────────┘  │  │  │
//! │  │  └─────────────────────────────────┘  │  │
//! │  └───────────────────────────────────────┘  │
//! └─────────────────────────────────────────────┘
//! ```

mod bridge;
mod connect;

pub use bridge::{BridgeLayer, BridgeService};
pub use connect::{ConnectLayer, ConnectService};
