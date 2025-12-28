//! Service builder for combining multiple Connect routers and gRPC services.
//!
//! This module provides [`MakeServiceBuilder`], a convenient builder for combining
//! multiple Connect RPC routers and optionally multiple Tonic gRPC services into
//! a single service.
//!
//! # Examples
//!
//! ## Connect-only (multiple routers)
//!
//! ```rust,no_run
//! use connectrpc_axum::MakeServiceBuilder;
//! # use axum::Router;
//! # let hello_router: Router<()> = Router::new();
//! # let user_router: Router<()> = Router::new();
//! # let team_router: Router<()> = Router::new();
//!
//! let app = MakeServiceBuilder::new()
//!     .add_router(hello_router)
//!     .add_routers(vec![user_router, team_router])
//!     .build();
//! ```
//!
//! ## Connect + gRPC (multiple services)
//!
//! Combine Connect routers and multiple gRPC services:
//!
//! ```rust,ignore
//! use connectrpc_axum::MakeServiceBuilder;
//! use axum::Router;
//!
//! let dispatch = MakeServiceBuilder::new()
//!     .add_routers(vec![hello_router, user_router])
//!     .add_grpc_service(hello_grpc_svc)
//!     .add_grpc_service(user_grpc_svc)
//!     .add_grpc_service(team_grpc_svc)
//!     .build();
//! ```
//!
//! **Note:** gRPC services are routed by their service name (from `NamedService::NAME`).
//! The builder uses `tonic::service::Routes` internally to handle multiple services.

use axum::Router;

use crate::context::MessageLimits;
use crate::layer::ConnectLayer;

#[cfg(feature = "tonic")]
use crate::tonic::ContentTypeSwitch;

/// Builder for combining multiple Connect routers and gRPC services.
///
/// This builder allows you to:
/// - Combine multiple Connect RPC routers into a single router
/// - Add multiple Tonic gRPC services (when `tonic` feature is enabled)
/// - Create a unified service that dispatches between gRPC and Connect protocols
///
/// # Type Parameters
///
/// - `S`: The state type for the routers (default: `()`)
///
/// # Examples
///
/// ```rust,no_run
/// use connectrpc_axum::MakeServiceBuilder;
/// # use axum::Router;
/// # let router1: Router<()> = Router::new();
/// # let router2: Router<()> = Router::new();
///
/// // Connect-only
/// let app = MakeServiceBuilder::new()
///     .add_router(router1)
///     .add_router(router2)
///     .build();
/// ```
///
/// ```rust,ignore
/// use connectrpc_axum::MakeServiceBuilder;
/// use axum::Router;
///
/// // Connect + multiple gRPC services
/// let app = MakeServiceBuilder::new()
///     .add_router(router1)
///     .add_grpc_service(grpc1)
///     .add_grpc_service(grpc2)
///     .build();
/// ```
pub struct MakeServiceBuilder<S = ()> {
    connect_router: Router<S>,
    #[cfg(feature = "tonic")]
    grpc_routes: tonic::service::Routes,
    /// Message size limits for requests
    limits: MessageLimits,
    /// Whether to require the Connect-Protocol-Version header
    require_protocol_header: bool,
}

impl<S> Default for MakeServiceBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> MakeServiceBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a new `MakeServiceBuilder`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use connectrpc_axum::MakeServiceBuilder;
    ///
    /// let builder: MakeServiceBuilder<()> = MakeServiceBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            connect_router: Router::new(),
            #[cfg(feature = "tonic")]
            grpc_routes: tonic::service::Routes::default(),
            limits: MessageLimits::default(),
            require_protocol_header: false,
        }
    }

    /// Set custom message size limits.
    ///
    /// Default is 4 MB.
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Require the `Connect-Protocol-Version` header on Connect protocol requests.
    ///
    /// When enabled, requests must include the `Connect-Protocol-Version: 1` header.
    /// This helps HTTP proxies and middleware identify valid Connect requests.
    ///
    /// Disabled by default to allow easy ad-hoc requests (e.g., with cURL).
    pub fn require_protocol_header(mut self, require: bool) -> Self {
        self.require_protocol_header = require;
        self
    }

    /// Adds a single Connect RPC router to the builder.
    ///
    /// The router will be merged with any previously added routers using
    /// [`Router::merge`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// # use axum::Router;
    /// # let hello_router: Router<()> = Router::new();
    ///
    /// let builder = MakeServiceBuilder::new()
    ///     .add_router(hello_router);
    /// ```
    pub fn add_router(mut self, router: Router<S>) -> Self {
        self.connect_router = self.connect_router.merge(router);
        self
    }

    /// Adds multiple Connect RPC routers to the builder.
    ///
    /// All routers will be merged together using [`Router::merge`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// # use axum::Router;
    /// # let router1: Router<()> = Router::new();
    /// # let router2: Router<()> = Router::new();
    /// # let router3: Router<()> = Router::new();
    ///
    /// let builder = MakeServiceBuilder::new()
    ///     .add_routers(vec![router1, router2, router3]);
    /// ```
    pub fn add_routers(mut self, routers: impl IntoIterator<Item = Router<S>>) -> Self {
        for router in routers {
            self.connect_router = self.connect_router.merge(router);
        }
        self
    }
}

// Tonic-specific methods (only available when tonic feature is enabled)
#[cfg(feature = "tonic")]
impl<S> MakeServiceBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Adds a gRPC service to the builder.
    ///
    /// This method can be called multiple times to add multiple gRPC services.
    /// Each service will be routed based on its service name from `NamedService::NAME`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::Router;
    ///
    /// let builder = MakeServiceBuilder::new()
    ///     .add_router(hello_router)
    ///     .add_grpc_service(hello_grpc_svc)
    ///     .add_grpc_service(user_grpc_svc);
    /// ```
    pub fn add_grpc_service<G>(mut self, svc: G) -> Self
    where
        G: tower::Service<http::Request<tonic::body::Body>, Error = std::convert::Infallible>
            + tonic::server::NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        G::Response: axum::response::IntoResponse,
        G::Future: Send + 'static,
    {
        self.grpc_routes = self.grpc_routes.add_service(svc);
        self
    }

    /// Builds a dispatch service that routes between gRPC and Connect protocols.
    ///
    /// This returns a [`ContentTypeSwitch`] service that inspects the `Content-Type`
    /// header and routes requests to either the gRPC services or Connect routers
    /// based on the protocol.
    ///
    /// - Requests with `content-type: application/grpc*` → routed to gRPC services
    /// - All other requests → routed to Connect routers
    ///
    /// The Connect router will have [`ConnectLayer`] applied with the configured
    /// message limits and protocol header requirements.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::Router;
    ///
    /// let dispatch = MakeServiceBuilder::new()
    ///     .add_routers(vec![hello_router, user_router])
    ///     .add_grpc_service(hello_grpc_svc)
    ///     .add_grpc_service(user_grpc_svc)
    ///     .build();
    ///
    /// // Serve with axum
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    /// axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
    /// ```
    pub fn build(self) -> ContentTypeSwitch<tonic::service::Routes, Router<S>> {
        let grpc_service = self.grpc_routes.prepare();
        let layer = ConnectLayer::new()
            .limits(self.limits)
            .require_protocol_header(self.require_protocol_header);
        let connect_router = self.connect_router.layer(layer);
        ContentTypeSwitch::new(grpc_service, connect_router)
    }
}

// Connect-only build method (when tonic feature is NOT enabled)
#[cfg(not(feature = "tonic"))]
impl<S> MakeServiceBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Builds a Connect-only router (no gRPC support).
    ///
    /// This returns the combined Connect RPC router containing all the routers
    /// that were added via [`add_router`](Self::add_router) or
    /// [`add_routers`](Self::add_routers).
    ///
    /// The router will have [`ConnectLayer`] applied with the configured
    /// message limits and protocol header requirements.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// # use axum::Router;
    /// # let router1: Router<()> = Router::new();
    /// # let router2: Router<()> = Router::new();
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(router1)
    ///     .add_router(router2)
    ///     .build();
    /// ```
    pub fn build(self) -> Router<S> {
        let layer = ConnectLayer::new()
            .limits(self.limits)
            .require_protocol_header(self.require_protocol_header);
        self.connect_router.layer(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    #[test]
    fn test_single_router() {
        let router: Router<()> = Router::new().route("/hello", get(|| async { "hello" }));

        let app = MakeServiceBuilder::new().add_router(router).build();

        // App should not be empty (has routes)
        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_multiple_routers_via_add_router() {
        let router1: Router<()> = Router::new().route("/hello", get(|| async { "hello" }));
        let router2: Router<()> = Router::new().route("/world", get(|| async { "world" }));

        let app = MakeServiceBuilder::new()
            .add_router(router1)
            .add_router(router2)
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_multiple_routers_via_add_routers() {
        let router1: Router<()> = Router::new().route("/hello", get(|| async { "hello" }));
        let router2: Router<()> = Router::new().route("/world", get(|| async { "world" }));
        let router3: Router<()> = Router::new().route("/test", get(|| async { "test" }));

        let app = MakeServiceBuilder::new()
            .add_routers(vec![router1, router2, router3])
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_combined_add_methods() {
        let router1: Router<()> = Router::new().route("/hello", get(|| async { "hello" }));
        let router2: Router<()> = Router::new().route("/world", get(|| async { "world" }));
        let router3: Router<()> = Router::new().route("/test", get(|| async { "test" }));

        let app = MakeServiceBuilder::new()
            .add_router(router1)
            .add_routers(vec![router2, router3])
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_default() {
        let _builder: MakeServiceBuilder = MakeServiceBuilder::default();
    }
}
