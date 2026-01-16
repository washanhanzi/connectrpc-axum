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
use http::StatusCode;
#[cfg(not(feature = "tonic"))]
use std::marker::PhantomData;
use std::time::Duration;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::SizeAbove;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use crate::context::{CompressionConfig, MessageLimits};
use crate::layer::{BridgeLayer, ConnectLayer};

#[cfg(feature = "tonic")]
use crate::tonic::ContentTypeSwitch;

/// Marker type indicating Connect-only mode (no gRPC services added).
pub struct ConnectOnly;

/// Marker type indicating Tonic-compatible mode (gRPC services added).
#[cfg(feature = "tonic")]
pub struct WithGrpc {
    routes: tonic::service::Routes,
    /// Whether to capture HTTP request parts for `FromRequestParts` extractors.
    /// Enabled by default.
    capture_request_parts: bool,
}

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
/// - `G`: The gRPC state marker (default: `ConnectOnly`)
///
/// # Return Types
///
/// The `build()` method returns different types based on whether gRPC services were added:
/// - Without gRPC services: Returns `Router<S>`
/// - With gRPC services: Returns `ContentTypeSwitch<tonic::service::Routes, Router<S>>`
///
/// # Examples
///
/// ```rust,no_run
/// use connectrpc_axum::MakeServiceBuilder;
/// # use axum::Router;
/// # let router1: Router<()> = Router::new();
/// # let router2: Router<()> = Router::new();
///
/// // Connect-only - returns Router<S>
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
/// // Connect + gRPC - returns ContentTypeSwitch
/// let app = MakeServiceBuilder::new()
///     .add_router(router1)
///     .add_grpc_service(grpc1)
///     .add_grpc_service(grpc2)
///     .build();
/// ```
pub struct MakeServiceBuilder<S = (), G = ConnectOnly> {
    connect_router: Router<S>,
    axum_router: Option<Router<S>>,
    raw_axum_router: Option<Router<S>>,
    #[cfg(feature = "tonic")]
    grpc_state: G,
    #[cfg(not(feature = "tonic"))]
    _grpc_state: PhantomData<G>,
    limits: MessageLimits,
    require_protocol_header: bool,
    compression: Option<CompressionConfig>,
    timeout: Option<Duration>,
}

impl<S> Default for MakeServiceBuilder<S, ConnectOnly>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> MakeServiceBuilder<S, ConnectOnly>
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
            axum_router: None,
            raw_axum_router: None,
            #[cfg(feature = "tonic")]
            grpc_state: ConnectOnly,
            #[cfg(not(feature = "tonic"))]
            _grpc_state: PhantomData,
            limits: MessageLimits::default(),
            require_protocol_header: false,
            compression: Some(CompressionConfig::default()),
            timeout: None,
        }
    }
}

impl<S, G> MakeServiceBuilder<S, G>
where
    S: Clone + Send + Sync + 'static,
{
    /// Set custom message size limits.
    ///
    /// Default is 4 MB for receive, no limit for send.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::{MakeServiceBuilder, MessageLimits};
    ///
    /// // Set both receive and send limits
    /// let app = MakeServiceBuilder::new()
    ///     .message_limits(
    ///         MessageLimits::default()
    ///             .receive_max_bytes(16 * 1024 * 1024)  // 16 MB for requests
    ///             .send_max_bytes(8 * 1024 * 1024)      // 8 MB for responses
    ///     )
    ///     .add_router(router)
    ///     .build();
    /// ```
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Set maximum size for incoming request messages (compressed body size).
    ///
    /// This limits the compressed request body size checked via `Content-Length` header.
    /// Requests exceeding this limit are rejected with `ResourceExhausted` error
    /// before decompression occurs.
    ///
    /// Default is 4 MB. Use `MessageLimits::unlimited()` via `message_limits()` for no limit.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    ///
    /// // Limit requests to 16 MB
    /// let app = MakeServiceBuilder::new()
    ///     .receive_max_bytes(16 * 1024 * 1024)
    ///     .add_router(router)
    ///     .build();
    /// ```
    pub fn receive_max_bytes(mut self, max: usize) -> Self {
        self.limits = self.limits.receive_max_bytes(max);
        self
    }

    /// Set maximum size for outgoing response messages.
    ///
    /// This limits the size of response messages. For unary RPCs, this checks the
    /// uncompressed size (before Tower compression). For streaming RPCs, this checks
    /// the compressed size of each message envelope.
    ///
    /// When a response would exceed this limit, a `ResourceExhausted` error
    /// is returned to the client, following connect-go's behavior.
    ///
    /// Default is unlimited (no limit).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    ///
    /// // Limit responses to 8 MB
    /// let app = MakeServiceBuilder::new()
    ///     .send_max_bytes(8 * 1024 * 1024)
    ///     .add_router(router)
    ///     .build();
    /// ```
    pub fn send_max_bytes(mut self, max: usize) -> Self {
        self.limits = self.limits.send_max_bytes(max);
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

    /// Set compression configuration.
    ///
    /// Controls response compression behavior:
    /// - `min_bytes`: Minimum response size before compression is applied (default: 0, matching connect-go)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::{MakeServiceBuilder, CompressionConfig};
    ///
    /// // Compress responses >= 512 bytes
    /// let app = MakeServiceBuilder::new()
    ///     .compression(CompressionConfig::new(512))
    ///     .add_router(router)
    ///     .build();
    /// ```
    pub fn compression(mut self, config: CompressionConfig) -> Self {
        self.compression = Some(config);
        self
    }

    /// Disable compression entirely.
    ///
    /// When disabled, no compression/decompression layers are added to the router.
    /// This is more efficient than setting a high threshold.
    pub fn disable_compression(mut self) -> Self {
        self.compression = None;
        self
    }

    /// Set the server-side maximum timeout.
    ///
    /// When set, the effective timeout for each request is the minimum of:
    /// - This server timeout
    /// - The client's `Connect-Timeout-Ms` header (if present)
    ///
    /// This ensures the smaller timeout always wins, matching Connect-Go's behavior.
    /// On timeout, a Connect `deadline_exceeded` error is returned.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    /// use connectrpc_axum::MakeServiceBuilder;
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .timeout(Duration::from_secs(30))
    ///     .add_router(router)
    ///     .build();
    /// ```
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
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

    /// Adds an axum router that bypasses [`ConnectLayer`].
    ///
    /// Use this for routes that don't need Connect protocol handling:
    /// - Health check endpoints
    /// - Metrics endpoints
    /// - Static file serving
    /// - Plain REST APIs
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::{Router, routing::get};
    /// # let connect_router: Router<()> = Router::new();
    ///
    /// let health_router = Router::new()
    ///     .route("/health", get(|| async { "ok" }));
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_axum_router(health_router)
    ///     .build();
    /// ```
    pub fn add_axum_router(mut self, router: Router<S>) -> Self {
        self.axum_router = Some(match self.axum_router {
            Some(existing) => existing.merge(router),
            None => router,
        });
        self
    }

    /// Adds multiple axum routers that bypass [`ConnectLayer`].
    ///
    /// All routers will be merged together and served without Connect protocol handling.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::{Router, routing::get};
    /// # let connect_router: Router<()> = Router::new();
    ///
    /// let health_router = Router::new()
    ///     .route("/health", get(|| async { "ok" }));
    /// let metrics_router = Router::new()
    ///     .route("/metrics", get(|| async { "metrics" }));
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_axum_routers(vec![health_router, metrics_router])
    ///     .build();
    /// ```
    pub fn add_axum_routers(mut self, routers: impl IntoIterator<Item = Router<S>>) -> Self {
        for router in routers {
            self.axum_router = Some(match self.axum_router {
                Some(existing) => existing.merge(router),
                None => router,
            });
        }
        self
    }

    /// Adds an axum router without any shared layers.
    ///
    /// Unlike [`add_axum_router`](Self::add_axum_router), routers added via this method
    /// bypass all shared configuration (compression, timeout, message limits).
    /// Use this for lightweight endpoints like health checks and metrics.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::{Router, routing::get};
    /// # let connect_router: Router<()> = Router::new();
    ///
    /// let health_router = Router::new()
    ///     .route("/health", get(|| async { "ok" }))
    ///     .route("/metrics", get(|| async { "metrics" }));
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_axum_router_raw(health_router)  // No compression, timeout, or limits
    ///     .build();
    /// ```
    pub fn add_axum_router_raw(mut self, router: Router<S>) -> Self {
        self.raw_axum_router = Some(match self.raw_axum_router {
            Some(existing) => existing.merge(router),
            None => router,
        });
        self
    }

    /// Adds multiple axum routers without any shared layers.
    ///
    /// All routers will be merged together and served without compression,
    /// timeout, or message limit layers.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::{Router, routing::get};
    /// # let connect_router: Router<()> = Router::new();
    ///
    /// let health_router = Router::new()
    ///     .route("/health", get(|| async { "ok" }));
    /// let metrics_router = Router::new()
    ///     .route("/metrics", get(|| async { "metrics" }));
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_axum_routers_raw(vec![health_router, metrics_router])
    ///     .build();
    /// ```
    pub fn add_axum_routers_raw(mut self, routers: impl IntoIterator<Item = Router<S>>) -> Self {
        for router in routers {
            self.raw_axum_router = Some(match self.raw_axum_router {
                Some(existing) => existing.merge(router),
                None => router,
            });
        }
        self
    }

    fn build_connect_layer(&self) -> ConnectLayer {
        let compression = self.compression.unwrap_or(CompressionConfig::disabled());
        let mut layer = ConnectLayer::new()
            .limits(self.limits)
            .require_protocol_header(self.require_protocol_header)
            .compression(compression);

        if let Some(timeout) = self.timeout {
            layer = layer.timeout(timeout);
        }

        layer
    }

    /// Builds a request decompression layer with feature-gated compression algorithms.
    ///
    /// By default, gzip is always enabled. Additional algorithms (deflate, br, zstd)
    /// are enabled based on cargo features.
    fn build_request_decompression_layer(&self) -> RequestDecompressionLayer {
        // gzip is always enabled (unconditional feature in Cargo.toml)
        let layer = RequestDecompressionLayer::new().gzip(true);

        #[cfg(feature = "compression-deflate")]
        let layer = layer.deflate(true);
        #[cfg(feature = "compression-br")]
        let layer = layer.br(true);
        #[cfg(feature = "compression-zstd")]
        let layer = layer.zstd(true);

        layer
    }
}

// Connect-only build method (no gRPC services added)
impl<S> MakeServiceBuilder<S, ConnectOnly>
where
    S: Clone + Send + Sync + 'static,
{
    /// Builds a Connect-only router.
    ///
    /// This returns the combined Connect RPC router containing all the routers
    /// that were added via [`add_router`](Self::add_router) or
    /// [`add_routers`](Self::add_routers), plus any axum routers added via
    /// [`add_axum_router`](Self::add_axum_router).
    ///
    /// The Connect routers will have [`ConnectLayer`] applied with the configured
    /// message limits and protocol header requirements. Axum routers bypass the
    /// Connect layer and are served as plain HTTP routes.
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
        let connect_layer = self.build_connect_layer();
        let request_decompression_layer = self.build_request_decompression_layer();

        let compression = self.compression;
        let limits = self.limits;
        let axum_router = self.axum_router;
        let raw_axum_router = self.raw_axum_router;
        let timeout = self.timeout;

        let router = self.connect_router.layer(connect_layer);

        let router = if let Some(compression) = compression {
            let min_bytes = compression.min_bytes.min(u16::MAX as usize) as u16;
            let compression_layer = CompressionLayer::new()
                .quality(compression.level)
                .compress_when(SizeAbove::new(min_bytes));
            let bridge_layer = BridgeLayer::with_receive_limit(limits.get_receive_max_bytes());

            let router = router
                .layer(compression_layer.clone())
                .layer(request_decompression_layer.clone())
                .layer(bridge_layer);

            match axum_router {
                Some(axum_router) => {
                    let axum_router = axum_router
                        .layer(compression_layer)
                        .layer(request_decompression_layer);
                    let axum_router = apply_axum_layers(axum_router, timeout, &limits);
                    router.merge(axum_router)
                }
                None => router,
            }
        } else {
            match axum_router {
                Some(axum_router) => {
                    let axum_router = apply_axum_layers(axum_router, timeout, &limits);
                    router.merge(axum_router)
                }
                None => router,
            }
        };

        match raw_axum_router {
            Some(raw) => router.merge(raw),
            None => router,
        }
    }
}

fn apply_axum_layers<S>(
    router: Router<S>,
    timeout: Option<Duration>,
    limits: &MessageLimits,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let router = match limits.get_receive_max_bytes() {
        Some(max_bytes) => router.layer(RequestBodyLimitLayer::new(max_bytes)),
        None => router,
    };
    match timeout {
        Some(timeout) => router.layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            timeout,
        )),
        None => router,
    }
}

// Tonic-specific methods (only available when tonic feature is enabled)
#[cfg(feature = "tonic")]
impl<S> MakeServiceBuilder<S, ConnectOnly>
where
    S: Clone + Send + Sync + 'static,
{
    /// Adds the first gRPC service to the builder.
    ///
    /// This transitions the builder to a state where `build()` will return
    /// a `ContentTypeSwitch` that dispatches between gRPC and Connect protocols.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use axum::Router;
    ///
    /// let (hello_router, hello_grpc) = HelloServiceWithGrpcBuilder::new()
    ///     .say_hello(handler)
    ///     .build();
    ///
    /// let builder = MakeServiceBuilder::new()
    ///     .add_router(hello_router)
    ///     .add_grpc_service(hello_grpc);
    /// ```
    pub fn add_grpc_service<G>(self, service: G) -> MakeServiceBuilder<S, WithGrpc>
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
        Self::add_grpc_service_with(self, service, |svc| svc)
    }

    /// Adds the first gRPC service with custom configuration.
    ///
    /// Use this to configure Tonic-specific options like compression on the gRPC service.
    /// The `configure` function receives the service and can apply Tonic's compression methods.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use tonic::codec::CompressionEncoding;
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_grpc_service_with(grpc_service, |svc| {
    ///         svc.accept_compressed(CompressionEncoding::Gzip)
    ///            .send_compressed(CompressionEncoding::Gzip)
    ///     })
    ///     .build();
    /// ```
    ///
    /// # Compression Note
    ///
    /// gRPC compression is separate from Connect compression:
    /// - **gRPC**: Configure via `accept_compressed`/`send_compressed` on the Tonic service
    /// - **Connect**: Configure via `MakeServiceBuilder::compression()` (uses Tower middleware)
    pub fn add_grpc_service_with<G, F>(
        self,
        service: G,
        configure: F,
    ) -> MakeServiceBuilder<S, WithGrpc>
    where
        G: tower::Service<http::Request<tonic::body::Body>, Error = std::convert::Infallible>
            + tonic::server::NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        G::Response: axum::response::IntoResponse,
        G::Future: Send + 'static,
        F: FnOnce(G) -> G,
    {
        let routes = tonic::service::Routes::default().add_service(configure(service));
        MakeServiceBuilder {
            connect_router: self.connect_router,
            axum_router: self.axum_router,
            raw_axum_router: self.raw_axum_router,
            grpc_state: WithGrpc {
                routes,
                capture_request_parts: true,
            },
            limits: self.limits,
            require_protocol_header: self.require_protocol_header,
            compression: self.compression,
            timeout: self.timeout,
        }
    }
}

#[cfg(feature = "tonic")]
impl<S> MakeServiceBuilder<S, WithGrpc>
where
    S: Clone + Send + Sync + 'static,
{
    /// Adds additional gRPC services to the builder.
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
    ///     .add_grpc_service(hello_grpc)
    ///     .add_grpc_service(user_grpc);
    /// ```
    pub fn add_grpc_service<G>(self, service: G) -> Self
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
        Self::add_grpc_service_with(self, service, |svc| svc)
    }

    /// Adds additional gRPC services with custom configuration.
    ///
    /// Use this to configure Tonic-specific options like compression on the gRPC service.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    /// use tonic::codec::CompressionEncoding;
    ///
    /// let app = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_grpc_service(first_grpc)
    ///     .add_grpc_service_with(second_grpc, |svc| {
    ///         svc.accept_compressed(CompressionEncoding::Gzip)
    ///            .send_compressed(CompressionEncoding::Gzip)
    ///     })
    ///     .build();
    /// ```
    pub fn add_grpc_service_with<G, F>(mut self, service: G, configure: F) -> Self
    where
        G: tower::Service<http::Request<tonic::body::Body>, Error = std::convert::Infallible>
            + tonic::server::NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        G::Response: axum::response::IntoResponse,
        G::Future: Send + 'static,
        F: FnOnce(G) -> G,
    {
        self.grpc_state.routes = self.grpc_state.routes.add_service(configure(service));
        self
    }

    /// Disable `FromRequestParts` extractor support for gRPC services.
    ///
    /// By default, `FromRequestPartsLayer` is applied to capture HTTP request parts
    /// (method, URI, headers, extensions) for use with `FromRequestParts` extractors
    /// in handlers. If your handlers don't use any extractors, you can disable this
    /// to avoid the overhead.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use connectrpc_axum::MakeServiceBuilder;
    ///
    /// let dispatch = MakeServiceBuilder::new()
    ///     .add_router(connect_router)
    ///     .add_grpc_service(grpc_server)
    ///     .without_from_request_parts()
    ///     .build();
    /// ```
    pub fn without_from_request_parts(mut self) -> Self {
        self.grpc_state.capture_request_parts = false;
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
    /// By default, `FromRequestPartsLayer` middleware is applied to the gRPC service
    /// to enable `FromRequestParts` extraction in handlers. Use
    /// [`without_from_request_parts()`](Self::without_from_request_parts) to disable this.
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
    ///     .add_grpc_service(hello_bundle)
    ///     .add_grpc_service(user_bundle)
    ///     .build();
    ///
    /// // Serve with axum
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    /// axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
    /// ```
    pub fn build(
        self,
    ) -> ContentTypeSwitch<
        impl tower::Service<
            http::Request<axum::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = std::convert::Infallible,
            Future = impl Send,
        > + Clone,
        Router<S>,
    > {
        use tower::ServiceBuilder;
        use tower::util::Either;

        let connect_layer = self.build_connect_layer();
        let request_decompression_layer = self.build_request_decompression_layer();

        let compression = self.compression;
        let limits = self.limits;
        let axum_router = self.axum_router;
        let raw_axum_router = self.raw_axum_router;
        let grpc_state = self.grpc_state;
        let timeout = self.timeout;

        let router = self.connect_router.layer(connect_layer);

        let connect_router = if let Some(compression) = compression {
            let min_bytes = compression.min_bytes.min(u16::MAX as usize) as u16;
            let compression_layer = CompressionLayer::new()
                .quality(compression.level)
                .compress_when(SizeAbove::new(min_bytes));
            let bridge_layer = BridgeLayer::with_receive_limit(limits.get_receive_max_bytes());

            let router = router
                .layer(compression_layer.clone())
                .layer(request_decompression_layer.clone())
                .layer(bridge_layer);

            match axum_router {
                Some(axum_router) => {
                    let axum_router = axum_router
                        .layer(compression_layer)
                        .layer(request_decompression_layer);
                    let axum_router = apply_axum_layers(axum_router, timeout, &limits);
                    router.merge(axum_router)
                }
                None => router,
            }
        } else {
            match axum_router {
                Some(axum_router) => {
                    let axum_router = apply_axum_layers(axum_router, timeout, &limits);
                    router.merge(axum_router)
                }
                None => router,
            }
        };

        let connect_router = match raw_axum_router {
            Some(raw) => connect_router.merge(raw),
            None => connect_router,
        };

        let grpc_routes = grpc_state.routes.prepare();
        let grpc_service = if grpc_state.capture_request_parts {
            Either::Left(
                ServiceBuilder::new()
                    .layer(crate::tonic::FromRequestPartsLayer::new())
                    .service(grpc_routes),
            )
        } else {
            Either::Right(grpc_routes)
        };
        ContentTypeSwitch::new(grpc_service, connect_router)
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

    #[test]
    fn test_axum_router() {
        let connect_router: Router<()> = Router::new().route("/rpc", get(|| async { "rpc" }));
        let axum_router: Router<()> = Router::new().route("/health", get(|| async { "ok" }));

        let app = MakeServiceBuilder::new()
            .add_router(connect_router)
            .add_axum_router(axum_router)
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_multiple_axum_routers() {
        let connect_router: Router<()> = Router::new().route("/rpc", get(|| async { "rpc" }));
        let health_router: Router<()> = Router::new().route("/health", get(|| async { "ok" }));
        let metrics_router: Router<()> =
            Router::new().route("/metrics", get(|| async { "metrics" }));

        let app = MakeServiceBuilder::new()
            .add_router(connect_router)
            .add_axum_routers(vec![health_router, metrics_router])
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_mixed_connect_and_axum_routers() {
        let connect_router1: Router<()> = Router::new().route("/rpc1", get(|| async { "rpc1" }));
        let connect_router2: Router<()> = Router::new().route("/rpc2", get(|| async { "rpc2" }));
        let axum_router1: Router<()> = Router::new().route("/health", get(|| async { "ok" }));
        let axum_router2: Router<()> = Router::new().route("/metrics", get(|| async { "metrics" }));

        let app = MakeServiceBuilder::new()
            .add_router(connect_router1)
            .add_axum_router(axum_router1)
            .add_router(connect_router2)
            .add_axum_router(axum_router2)
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_axum_router_raw() {
        let connect_router: Router<()> = Router::new().route("/rpc", get(|| async { "rpc" }));
        let health_router: Router<()> = Router::new().route("/health", get(|| async { "ok" }));

        let app = MakeServiceBuilder::new()
            .add_router(connect_router)
            .add_axum_router_raw(health_router)
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_multiple_axum_routers_raw() {
        let connect_router: Router<()> = Router::new().route("/rpc", get(|| async { "rpc" }));
        let health_router: Router<()> = Router::new().route("/health", get(|| async { "ok" }));
        let metrics_router: Router<()> =
            Router::new().route("/metrics", get(|| async { "metrics" }));

        let app = MakeServiceBuilder::new()
            .add_router(connect_router)
            .add_axum_routers_raw(vec![health_router, metrics_router])
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }

    #[test]
    fn test_mixed_axum_and_raw_routers() {
        let connect_router: Router<()> = Router::new().route("/rpc", get(|| async { "rpc" }));
        let rest_router: Router<()> = Router::new().route("/api/users", get(|| async { "users" }));
        let health_router: Router<()> = Router::new().route("/health", get(|| async { "ok" }));

        let app = MakeServiceBuilder::new()
            .add_router(connect_router)
            .add_axum_router(rest_router)
            .add_axum_router_raw(health_router)
            .build();

        assert!(format!("{:?}", app).contains("Router"));
    }
}
