//! Unified interceptor system for Connect RPC client.
//!
//! Two user-facing traits:
//! - [`Interceptor`]: Header-level access only (simple, no message bounds)
//! - [`MessageInterceptor`]: Full typed message access
//!
//! Both are wrapped internally to a unified [`InterceptorInternal`] trait,
//! enabling zero-cost composition via [`Chain`].
//!
//! # Example
//!
//! ```ignore
//! use connectrpc_axum_client::{Interceptor, MessageInterceptor, RequestContext};
//!
//! // Header-only interceptor - simple
//! #[derive(Clone)]
//! struct AuthInterceptor { token: String }
//!
//! impl Interceptor for AuthInterceptor {
//!     fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
//!         ctx.headers.insert("authorization", self.token.parse().unwrap());
//!         Ok(())
//!     }
//! }
//!
//! // Message interceptor - full typed access
//! #[derive(Clone)]
//! struct LoggingInterceptor;
//!
//! impl MessageInterceptor for LoggingInterceptor {
//!     fn on_request<Req>(&self, ctx: &mut RequestContext, req: &mut Req) -> Result<(), ClientError>
//!     where
//!         Req: prost::Message + serde::Serialize + 'static,
//!     {
//!         println!("Calling {} with {} bytes", ctx.procedure, req.encoded_len());
//!         Ok(())
//!     }
//! }
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .with_interceptor(AuthInterceptor { token: "Bearer xyz".into() })
//!     .with_message_interceptor(LoggingInterceptor)
//!     .build()?;
//! ```

use std::sync::Arc;

use http::HeaderMap;
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};

use crate::ClientError;

// ============================================================================
// Context Types
// ============================================================================

/// Context for intercepting a request.
///
/// Provides access to the procedure name and mutable headers.
#[derive(Debug)]
pub struct RequestContext<'a> {
    /// The procedure being called (e.g., "package.Service/Method").
    pub procedure: &'a str,
    /// HTTP headers for the request (mutable).
    pub headers: &'a mut HeaderMap,
}

impl<'a> RequestContext<'a> {
    /// Create a new request context.
    pub fn new(procedure: &'a str, headers: &'a mut HeaderMap) -> Self {
        Self { procedure, headers }
    }
}

/// Context for intercepting a response.
///
/// Provides access to the procedure name and response headers (read-only).
#[derive(Debug)]
pub struct ResponseContext<'a> {
    /// The procedure being called (e.g., "package.Service/Method").
    pub procedure: &'a str,
    /// HTTP headers from the response.
    pub headers: &'a HeaderMap,
}

impl<'a> ResponseContext<'a> {
    /// Create a new response context.
    pub fn new(procedure: &'a str, headers: &'a HeaderMap) -> Self {
        Self { procedure, headers }
    }
}

/// Context for intercepting streaming messages.
///
/// Provides access to the procedure name, stream type, and headers.
#[derive(Debug)]
pub struct StreamContext<'a> {
    /// The procedure being called (e.g., "package.Service/Method").
    pub procedure: &'a str,
    /// The type of stream (client, server, or bidirectional).
    pub stream_type: StreamType,
    /// HTTP headers from the initial request.
    pub request_headers: &'a HeaderMap,
    /// HTTP headers from the response (available after first response).
    pub response_headers: Option<&'a HeaderMap>,
}

impl<'a> StreamContext<'a> {
    /// Create a new stream context.
    pub fn new(
        procedure: &'a str,
        stream_type: StreamType,
        request_headers: &'a HeaderMap,
        response_headers: Option<&'a HeaderMap>,
    ) -> Self {
        Self {
            procedure,
            stream_type,
            request_headers,
            response_headers,
        }
    }
}

/// The type of streaming RPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamType {
    /// Client streams multiple requests, server sends one response.
    ClientStream,
    /// Client sends one request, server streams multiple responses.
    ServerStream,
    /// Both client and server stream messages.
    BidiStream,
}

// ============================================================================
// User-Facing Traits
// ============================================================================

/// Header-level interceptor - simple, no message access.
///
/// Use this for:
/// - Adding authentication headers
/// - Adding trace/correlation IDs
/// - Logging procedure names
/// - Any cross-cutting concern that only needs header access
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{Interceptor, RequestContext, ClientError};
///
/// #[derive(Clone)]
/// struct AuthInterceptor {
///     token: String,
/// }
///
/// impl Interceptor for AuthInterceptor {
///     fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
///         ctx.headers.insert("authorization", self.token.parse().unwrap());
///         Ok(())
///     }
/// }
/// ```
pub trait Interceptor: Send + Sync + Clone + 'static {
    /// Called before the request is sent.
    ///
    /// Can modify headers or return an error to abort the call.
    fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
        let _ = ctx;
        Ok(())
    }

    /// Called after the response is received.
    ///
    /// Can inspect response headers.
    fn on_response(&self, ctx: &ResponseContext) -> Result<(), ClientError> {
        let _ = ctx;
        Ok(())
    }
}

/// Message-level interceptor - full typed access to request/response bodies.
///
/// Use this for:
/// - Validating request fields before sending
/// - Transforming messages
/// - Logging message contents
/// - Per-message interception in streaming RPCs
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{MessageInterceptor, RequestContext, ClientError};
/// use prost::Message;
///
/// #[derive(Clone)]
/// struct LoggingInterceptor;
///
/// impl MessageInterceptor for LoggingInterceptor {
///     fn on_request<Req>(
///         &self,
///         ctx: &mut RequestContext,
///         request: &mut Req,
///     ) -> Result<(), ClientError>
///     where
///         Req: Message + serde::Serialize + 'static,
///     {
///         println!("Calling {} with {} bytes", ctx.procedure, request.encoded_len());
///         Ok(())
///     }
/// }
/// ```
pub trait MessageInterceptor: Send + Sync + Clone + 'static {
    /// Called before a unary request is sent.
    fn on_request<Req>(
        &self,
        ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        let _ = (ctx, request);
        Ok(())
    }

    /// Called after a unary response is received.
    fn on_response<Res>(
        &self,
        ctx: &ResponseContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        let _ = (ctx, response);
        Ok(())
    }

    /// Called before sending a message on a stream.
    fn on_stream_send<Req>(
        &self,
        ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        let _ = (ctx, request);
        Ok(())
    }

    /// Called after receiving a message from a stream.
    fn on_stream_receive<Res>(
        &self,
        ctx: &StreamContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        let _ = (ctx, response);
        Ok(())
    }
}

// ============================================================================
// Internal Unified Trait
// ============================================================================

/// Internal trait that unifies both interceptor types.
///
/// Not intended for direct implementation - use [`Interceptor`] or
/// [`MessageInterceptor`] instead.
pub trait InterceptorInternal: Send + Sync + Clone + 'static {
    /// Intercept a unary request.
    fn intercept_request<Req>(
        &self,
        ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static;

    /// Intercept a unary response.
    fn intercept_response<Res>(
        &self,
        ctx: &ResponseContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static;

    /// Intercept a stream send.
    fn intercept_stream_send<Req>(
        &self,
        ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static;

    /// Intercept a stream receive.
    fn intercept_stream_receive<Res>(
        &self,
        ctx: &StreamContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static;
}

// ============================================================================
// Base Case: Unit Type
// ============================================================================

/// The unit type implements all interceptor traits as no-ops,
/// serving as the base case for generic interceptor chains.
impl Interceptor for () {}
impl MessageInterceptor for () {}

impl InterceptorInternal for () {
    #[inline]
    fn intercept_request<Req>(
        &self,
        _ctx: &mut RequestContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        Ok(())
    }

    #[inline]
    fn intercept_response<Res>(
        &self,
        _ctx: &ResponseContext,
        _response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        Ok(())
    }

    #[inline]
    fn intercept_stream_send<Req>(
        &self,
        _ctx: &StreamContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        Ok(())
    }

    #[inline]
    fn intercept_stream_receive<Res>(
        &self,
        _ctx: &StreamContext,
        _response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        Ok(())
    }
}

// ============================================================================
// Wrappers
// ============================================================================

/// Wrapper that adapts a header-level [`Interceptor`] to [`InterceptorInternal`].
#[derive(Clone, Debug)]
pub struct HeaderWrapper<I>(pub I);

impl<I: Interceptor> InterceptorInternal for HeaderWrapper<I> {
    fn intercept_request<Req>(
        &self,
        ctx: &mut RequestContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        self.0.on_request(ctx)
    }

    fn intercept_response<Res>(
        &self,
        ctx: &ResponseContext,
        _response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        self.0.on_response(ctx)
    }

    fn intercept_stream_send<Req>(
        &self,
        _ctx: &StreamContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        // Header interceptors don't intercept individual stream messages
        Ok(())
    }

    fn intercept_stream_receive<Res>(
        &self,
        _ctx: &StreamContext,
        _response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        Ok(())
    }
}

/// Wrapper that adapts a [`MessageInterceptor`] to [`InterceptorInternal`].
#[derive(Clone, Debug)]
pub struct MessageWrapper<I>(pub I);

impl<I: MessageInterceptor> InterceptorInternal for MessageWrapper<I> {
    fn intercept_request<Req>(
        &self,
        ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        self.0.on_request(ctx, request)
    }

    fn intercept_response<Res>(
        &self,
        ctx: &ResponseContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        self.0.on_response(ctx, response)
    }

    fn intercept_stream_send<Req>(
        &self,
        ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        self.0.on_stream_send(ctx, request)
    }

    fn intercept_stream_receive<Res>(
        &self,
        ctx: &StreamContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        self.0.on_stream_receive(ctx, response)
    }
}

// ============================================================================
// Chain Combinator
// ============================================================================

/// A compile-time chain of two interceptors.
///
/// `Chain<A, B>` applies interceptor `A` first, then `B` for requests.
/// For responses, they are applied in reverse order (`B` then `A`),
/// following the middleware unwinding pattern.
///
/// This enables zero-cost interceptor composition without dynamic dispatch.
#[derive(Clone, Debug)]
pub struct Chain<A, B>(pub A, pub B);

impl<A, B> InterceptorInternal for Chain<A, B>
where
    A: InterceptorInternal,
    B: InterceptorInternal,
{
    fn intercept_request<Req>(
        &self,
        ctx: &mut RequestContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        self.0.intercept_request(ctx, request)?;
        self.1.intercept_request(ctx, request)
    }

    fn intercept_response<Res>(
        &self,
        ctx: &ResponseContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        // Reverse order for responses (middleware unwinding)
        self.1.intercept_response(ctx, response)?;
        self.0.intercept_response(ctx, response)
    }

    fn intercept_stream_send<Req>(
        &self,
        ctx: &StreamContext,
        request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        self.0.intercept_stream_send(ctx, request)?;
        self.1.intercept_stream_send(ctx, request)
    }

    fn intercept_stream_receive<Res>(
        &self,
        ctx: &StreamContext,
        response: &mut Res,
    ) -> Result<(), ClientError>
    where
        Res: Message + DeserializeOwned + Default + 'static,
    {
        // Reverse order for responses
        self.1.intercept_stream_receive(ctx, response)?;
        self.0.intercept_stream_receive(ctx, response)
    }
}

// ============================================================================
// Convenience Types
// ============================================================================

/// A simple interceptor that adds a header to all requests.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::HeaderInterceptor;
///
/// let auth = HeaderInterceptor::new("authorization", "Bearer token123");
/// let client = ConnectClient::builder("http://localhost:3000")
///     .with_interceptor(auth)
///     .build()?;
/// ```
#[derive(Clone, Debug)]
pub struct HeaderInterceptor {
    name: http::HeaderName,
    value: http::HeaderValue,
}

impl HeaderInterceptor {
    /// Create a new header interceptor.
    ///
    /// # Panics
    ///
    /// Panics if the header name or value is invalid.
    pub fn new(name: &str, value: &str) -> Self {
        Self {
            name: name.parse().expect("invalid header name"),
            value: value.parse().expect("invalid header value"),
        }
    }

    /// Try to create a new header interceptor, returning an error if invalid.
    pub fn try_new(name: &str, value: &str) -> Result<Self, ClientError> {
        let name = name
            .parse()
            .map_err(|_| ClientError::Protocol(format!("invalid header name: {}", name)))?;
        let value = value
            .parse()
            .map_err(|_| ClientError::Protocol(format!("invalid header value: {}", value)))?;
        Ok(Self { name, value })
    }

    /// Create a new header interceptor from pre-parsed values.
    pub fn from_parts(name: http::HeaderName, value: http::HeaderValue) -> Self {
        Self { name, value }
    }
}

impl Interceptor for HeaderInterceptor {
    fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
        ctx.headers.insert(self.name.clone(), self.value.clone());
        Ok(())
    }
}

/// A closure-based interceptor for quick header-level interception.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::ClosureInterceptor;
///
/// let logging = ClosureInterceptor::new(|ctx| {
///     println!("Calling: {}", ctx.procedure);
///     Ok(())
/// });
/// ```
#[derive(Clone)]
pub struct ClosureInterceptor<F> {
    on_request: F,
}

impl<F> ClosureInterceptor<F>
where
    F: Fn(&mut RequestContext) -> Result<(), ClientError> + Send + Sync + Clone + 'static,
{
    /// Create a new closure interceptor.
    pub fn new(on_request: F) -> Self {
        Self { on_request }
    }
}

impl<F> std::fmt::Debug for ClosureInterceptor<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosureInterceptor").finish()
    }
}

impl<F> Interceptor for ClosureInterceptor<F>
where
    F: Fn(&mut RequestContext) -> Result<(), ClientError> + Send + Sync + Clone + 'static,
{
    fn on_request(&self, ctx: &mut RequestContext) -> Result<(), ClientError> {
        (self.on_request)(ctx)
    }
}

// ============================================================================
// Typed Per-Method Interceptors
// ============================================================================

/// Typed interceptor with mutable context - used for "before" interception.
///
/// This trait allows intercepting requests before they are sent, with full
/// type safety on the message body. The context is mutable, allowing
/// modification of request headers.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{TypedMutInterceptor, RequestContext, ClientError};
///
/// struct ValidateSentence;
///
/// impl TypedMutInterceptor<SayRequest> for ValidateSentence {
///     fn intercept(&self, ctx: &mut RequestContext<'_>, body: &mut SayRequest) -> Result<(), ClientError> {
///         if body.sentence.is_empty() {
///             return Err(ClientError::invalid_argument("sentence required"));
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait TypedMutInterceptor<Body>: Send + Sync + 'static {
    /// Intercept the request before it is sent.
    ///
    /// Can modify headers via `ctx` or the request body directly.
    /// Return an error to abort the RPC.
    fn intercept(&self, ctx: &mut RequestContext<'_>, body: &mut Body) -> Result<(), ClientError>;
}

/// Typed interceptor with immutable context - used for "after", "on_send", "on_receive".
///
/// This trait allows intercepting responses or stream messages with full
/// type safety. The context is immutable since headers cannot be modified
/// after the request is sent.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{TypedInterceptor, ResponseContext, ClientError};
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// struct ResponseCounter {
///     count: Arc<AtomicUsize>,
/// }
///
/// impl TypedInterceptor<ResponseContext<'_>, SayResponse> for ResponseCounter {
///     fn intercept(&self, ctx: &ResponseContext<'_>, body: &mut SayResponse) -> Result<(), ClientError> {
///         self.count.fetch_add(1, Ordering::Relaxed);
///         Ok(())
///     }
/// }
/// ```
pub trait TypedInterceptor<Ctx, Body>: Send + Sync + 'static {
    /// Intercept the message.
    ///
    /// Can inspect context and modify the body.
    /// Return an error to signal failure.
    fn intercept(&self, ctx: &Ctx, body: &mut Body) -> Result<(), ClientError>;
}

// ============================================================================
// Blanket Implementations for Closures
// ============================================================================

impl<Body, F> TypedMutInterceptor<Body> for F
where
    Body: 'static,
    F: Fn(&mut RequestContext<'_>, &mut Body) -> Result<(), ClientError> + Send + Sync + 'static,
{
    fn intercept(&self, ctx: &mut RequestContext<'_>, body: &mut Body) -> Result<(), ClientError> {
        self(ctx, body)
    }
}

impl<Ctx, Body, F> TypedInterceptor<Ctx, Body> for F
where
    Body: 'static,
    F: Fn(&Ctx, &mut Body) -> Result<(), ClientError> + Send + Sync + 'static,
{
    fn intercept(&self, ctx: &Ctx, body: &mut Body) -> Result<(), ClientError> {
        self(ctx, body)
    }
}

// ============================================================================
// HRTB Closure Coercion Helpers
// ============================================================================

/// Coerce a closure into a response interceptor with the correct HRTB bounds.
///
/// Rust's type inference sometimes fails to infer the higher-ranked lifetime
/// when using closures as `for<'a> TypedInterceptor<ResponseContext<'a>, T>`.
/// This helper forces the correct inference.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::response_interceptor;
///
/// let client = MyServiceClient::builder("http://localhost:3000")
///     .with_after_my_method(response_interceptor(|ctx, resp: &mut MyResponse| {
///         resp.message.push_str(" [intercepted]");
///         Ok(())
///     }))
///     .build()?;
/// ```
pub fn response_interceptor<Body, F>(f: F) -> F
where
    Body: 'static,
    F: for<'a> Fn(&ResponseContext<'a>, &mut Body) -> Result<(), ClientError>
        + Send
        + Sync
        + 'static,
{
    f
}

/// Coerce a closure into a stream interceptor with the correct HRTB bounds.
///
/// Rust's type inference sometimes fails to infer the higher-ranked lifetime
/// when using closures as `for<'a> TypedInterceptor<StreamContext<'a>, T>`.
/// This helper forces the correct inference.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::stream_interceptor;
///
/// let client = MyServiceClient::builder("http://localhost:3000")
///     .with_on_receive_my_stream(stream_interceptor(|ctx, msg: &mut MyResponse| {
///         println!("Received: {:?}", msg);
///         Ok(())
///     }))
///     .build()?;
/// ```
pub fn stream_interceptor<Body, F>(f: F) -> F
where
    Body: 'static,
    F: for<'a> Fn(&StreamContext<'a>, &mut Body) -> Result<(), ClientError>
        + Send
        + Sync
        + 'static,
{
    f
}

// ============================================================================
// Per-RPC Type Interceptor Storage
// ============================================================================

/// Interceptors for unary RPCs: single request, single response.
///
/// # Example
///
/// ```ignore
/// let interceptors = UnaryInterceptors {
///     before: Some(Arc::new(|ctx: &mut RequestContext<'_>, req: &mut MyRequest| {
///         ctx.headers.insert("x-validated", "true".parse().unwrap());
///         Ok(())
///     })),
///     after: Some(Arc::new(|ctx: &ResponseContext<'_>, res: &mut MyResponse| {
///         println!("Got response: {:?}", res);
///         Ok(())
///     })),
/// };
/// ```
pub struct UnaryInterceptors<Req, Res> {
    /// Called before the request is sent. Can modify headers and request body.
    pub before: Option<Arc<dyn TypedMutInterceptor<Req>>>,
    /// Called after the response is received. Can inspect/modify response body.
    pub after: Option<Arc<dyn for<'a> TypedInterceptor<ResponseContext<'a>, Res>>>,
}

impl<Req, Res> Default for UnaryInterceptors<Req, Res> {
    fn default() -> Self {
        Self {
            before: None,
            after: None,
        }
    }
}

impl<Req, Res> Clone for UnaryInterceptors<Req, Res> {
    fn clone(&self) -> Self {
        Self {
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }
}

impl<Req, Res> std::fmt::Debug for UnaryInterceptors<Req, Res> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnaryInterceptors")
            .field("before", &self.before.as_ref().map(|_| "..."))
            .field("after", &self.after.as_ref().map(|_| "..."))
            .finish()
    }
}

/// Interceptors for server streaming RPCs: single request, stream of responses.
///
/// # Example
///
/// ```ignore
/// let interceptors = ServerStreamInterceptors {
///     before: Some(Arc::new(|ctx: &mut RequestContext<'_>, req: &mut ListRequest| {
///         Ok(())
///     })),
///     on_receive: Some(Arc::new(|ctx: &StreamContext<'_>, item: &mut ListItem| {
///         println!("Received item: {:?}", item);
///         Ok(())
///     })),
/// };
/// ```
pub struct ServerStreamInterceptors<Req, Res> {
    /// Called before the request is sent. Can modify headers and request body.
    pub before: Option<Arc<dyn TypedMutInterceptor<Req>>>,
    /// Called for each message received from the server stream.
    pub on_receive: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, Res>>>,
}

impl<Req, Res> Default for ServerStreamInterceptors<Req, Res> {
    fn default() -> Self {
        Self {
            before: None,
            on_receive: None,
        }
    }
}

impl<Req, Res> Clone for ServerStreamInterceptors<Req, Res> {
    fn clone(&self) -> Self {
        Self {
            before: self.before.clone(),
            on_receive: self.on_receive.clone(),
        }
    }
}

impl<Req, Res> std::fmt::Debug for ServerStreamInterceptors<Req, Res> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerStreamInterceptors")
            .field("before", &self.before.as_ref().map(|_| "..."))
            .field("on_receive", &self.on_receive.as_ref().map(|_| "..."))
            .finish()
    }
}

/// Interceptors for client streaming RPCs: stream of requests, single response.
///
/// # Example
///
/// ```ignore
/// let interceptors = ClientStreamInterceptors {
///     on_send: Some(Arc::new(|ctx: &StreamContext<'_>, msg: &mut UploadChunk| {
///         println!("Sending chunk: {} bytes", msg.data.len());
///         Ok(())
///     })),
///     after: Some(Arc::new(|ctx: &ResponseContext<'_>, res: &mut UploadResponse| {
///         Ok(())
///     })),
/// };
/// ```
pub struct ClientStreamInterceptors<Req, Res> {
    /// Called for each message before it is sent to the server.
    pub on_send: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, Req>>>,
    /// Called after the final response is received.
    pub after: Option<Arc<dyn for<'a> TypedInterceptor<ResponseContext<'a>, Res>>>,
}

impl<Req, Res> Default for ClientStreamInterceptors<Req, Res> {
    fn default() -> Self {
        Self {
            on_send: None,
            after: None,
        }
    }
}

impl<Req, Res> Clone for ClientStreamInterceptors<Req, Res> {
    fn clone(&self) -> Self {
        Self {
            on_send: self.on_send.clone(),
            after: self.after.clone(),
        }
    }
}

impl<Req, Res> std::fmt::Debug for ClientStreamInterceptors<Req, Res> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientStreamInterceptors")
            .field("on_send", &self.on_send.as_ref().map(|_| "..."))
            .field("after", &self.after.as_ref().map(|_| "..."))
            .finish()
    }
}

/// Interceptors for bidirectional streaming RPCs: stream of requests, stream of responses.
///
/// # Example
///
/// ```ignore
/// let interceptors = BidiStreamInterceptors {
///     on_send: Some(Arc::new(|ctx: &StreamContext<'_>, msg: &mut ChatMessage| {
///         println!("Sending: {}", msg.text);
///         Ok(())
///     })),
///     on_receive: Some(Arc::new(|ctx: &StreamContext<'_>, msg: &mut ChatMessage| {
///         println!("Received: {}", msg.text);
///         Ok(())
///     })),
/// };
/// ```
pub struct BidiStreamInterceptors<Req, Res> {
    /// Called for each message before it is sent to the server.
    pub on_send: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, Req>>>,
    /// Called for each message received from the server stream.
    pub on_receive: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, Res>>>,
}

impl<Req, Res> Default for BidiStreamInterceptors<Req, Res> {
    fn default() -> Self {
        Self {
            on_send: None,
            on_receive: None,
        }
    }
}

impl<Req, Res> Clone for BidiStreamInterceptors<Req, Res> {
    fn clone(&self) -> Self {
        Self {
            on_send: self.on_send.clone(),
            on_receive: self.on_receive.clone(),
        }
    }
}

impl<Req, Res> std::fmt::Debug for BidiStreamInterceptors<Req, Res> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BidiStreamInterceptors")
            .field("on_send", &self.on_send.as_ref().map(|_| "..."))
            .field("on_receive", &self.on_receive.as_ref().map(|_| "..."))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Test message type
    #[derive(Clone, Default, PartialEq)]
    struct TestMessage {
        value: String,
    }

    impl std::fmt::Debug for TestMessage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TestMessage")
                .field("value", &self.value)
                .finish()
        }
    }

    impl serde::Serialize for TestMessage {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use serde::ser::SerializeStruct;
            let mut state = serializer.serialize_struct("TestMessage", 1)?;
            state.serialize_field("value", &self.value)?;
            state.end()
        }
    }

    impl<'de> serde::Deserialize<'de> for TestMessage {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            #[derive(serde::Deserialize)]
            struct Helper {
                value: String,
            }
            let helper = Helper::deserialize(deserializer)?;
            Ok(TestMessage {
                value: helper.value,
            })
        }
    }

    impl prost::Message for TestMessage {
        fn encode_raw(&self, buf: &mut impl bytes::BufMut)
        where
            Self: Sized,
        {
            if !self.value.is_empty() {
                prost::encoding::string::encode(1, &self.value, buf);
            }
        }

        fn merge_field(
            &mut self,
            tag: u32,
            wire_type: prost::encoding::WireType,
            buf: &mut impl bytes::Buf,
            ctx: prost::encoding::DecodeContext,
        ) -> Result<(), prost::DecodeError>
        where
            Self: Sized,
        {
            if tag == 1 {
                prost::encoding::string::merge(wire_type, &mut self.value, buf, ctx)
            } else {
                prost::encoding::skip_field(wire_type, tag, buf, ctx)
            }
        }

        fn encoded_len(&self) -> usize {
            if self.value.is_empty() {
                0
            } else {
                prost::encoding::string::encoded_len(1, &self.value)
            }
        }

        fn clear(&mut self) {
            self.value.clear();
        }
    }

    // ========================================================================
    // Header Interceptor Tests
    // ========================================================================

    #[test]
    fn test_header_interceptor() {
        let interceptor = HeaderInterceptor::new("x-custom", "value");
        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);

        interceptor.on_request(&mut ctx).unwrap();
        assert_eq!(headers.get("x-custom").unwrap(), "value");
    }

    #[test]
    fn test_closure_interceptor() {
        let interceptor = ClosureInterceptor::new(|ctx: &mut RequestContext| {
            ctx.headers.insert("x-closure", "test".parse().unwrap());
            Ok(())
        });

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);

        interceptor.on_request(&mut ctx).unwrap();
        assert_eq!(headers.get("x-closure").unwrap(), "test");
    }

    // ========================================================================
    // Wrapper Tests
    // ========================================================================

    #[test]
    fn test_header_wrapper_ignores_message() {
        let interceptor = HeaderInterceptor::new("x-header", "value");
        let wrapped = HeaderWrapper(interceptor);

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage {
            value: "original".into(),
        };

        wrapped.intercept_request(&mut ctx, &mut msg).unwrap();

        // Header was added
        assert_eq!(headers.get("x-header").unwrap(), "value");
        // Message was not modified
        assert_eq!(msg.value, "original");
    }

    #[test]
    fn test_message_wrapper_receives_message() {
        #[derive(Clone)]
        struct ModifyingInterceptor;

        impl MessageInterceptor for ModifyingInterceptor {
            fn on_request<Req>(
                &self,
                _ctx: &mut RequestContext,
                request: &mut Req,
            ) -> Result<(), ClientError>
            where
                Req: Message + Serialize + 'static,
            {
                use std::any::Any;
                if let Some(msg) = (request as &mut dyn Any).downcast_mut::<TestMessage>() {
                    msg.value = format!("modified: {}", msg.value);
                }
                Ok(())
            }
        }

        let wrapped = MessageWrapper(ModifyingInterceptor);

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage {
            value: "original".into(),
        };

        wrapped.intercept_request(&mut ctx, &mut msg).unwrap();
        assert_eq!(msg.value, "modified: original");
    }

    // ========================================================================
    // Chain Tests
    // ========================================================================

    #[test]
    fn test_chain_header_and_message_interceptors() {
        // Header interceptor
        let header = HeaderWrapper(HeaderInterceptor::new("x-auth", "token"));

        // Message interceptor that logs encoded length
        #[derive(Clone)]
        struct LengthLogger {
            lengths: Arc<std::sync::Mutex<Vec<usize>>>,
        }

        impl MessageInterceptor for LengthLogger {
            fn on_request<Req>(
                &self,
                _ctx: &mut RequestContext,
                request: &mut Req,
            ) -> Result<(), ClientError>
            where
                Req: Message + Serialize + 'static,
            {
                self.lengths.lock().unwrap().push(request.encoded_len());
                Ok(())
            }
        }

        let lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let message = MessageWrapper(LengthLogger {
            lengths: lengths.clone(),
        });

        // Chain them: header first, then message
        let chain = Chain(header, message);

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage {
            value: "hello".into(),
        };

        chain.intercept_request(&mut ctx, &mut msg).unwrap();

        // Both interceptors were called
        assert_eq!(headers.get("x-auth").unwrap(), "token");
        assert_eq!(lengths.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_chain_order_for_requests() {
        let order = Arc::new(AtomicUsize::new(0));

        #[derive(Clone)]
        struct OrderTracker {
            name: &'static str,
            order: Arc<AtomicUsize>,
            expected: usize,
        }

        impl MessageInterceptor for OrderTracker {
            fn on_request<Req>(
                &self,
                _ctx: &mut RequestContext,
                _request: &mut Req,
            ) -> Result<(), ClientError>
            where
                Req: Message + Serialize + 'static,
            {
                let current = self.order.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    current, self.expected,
                    "{} called at wrong order",
                    self.name
                );
                Ok(())
            }
        }

        let first = MessageWrapper(OrderTracker {
            name: "first",
            order: order.clone(),
            expected: 0,
        });
        let second = MessageWrapper(OrderTracker {
            name: "second",
            order: order.clone(),
            expected: 1,
        });

        let chain = Chain(first, second);

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage::default();

        chain.intercept_request(&mut ctx, &mut msg).unwrap();
    }

    #[test]
    fn test_chain_reverse_order_for_responses() {
        let order = Arc::new(AtomicUsize::new(0));

        #[derive(Clone)]
        struct OrderTracker {
            name: &'static str,
            order: Arc<AtomicUsize>,
            expected: usize,
        }

        impl MessageInterceptor for OrderTracker {
            fn on_response<Res>(
                &self,
                _ctx: &ResponseContext,
                _response: &mut Res,
            ) -> Result<(), ClientError>
            where
                Res: Message + DeserializeOwned + Default + 'static,
            {
                let current = self.order.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    current, self.expected,
                    "{} called at wrong order",
                    self.name
                );
                Ok(())
            }
        }

        // For responses, second should be called first (reverse order)
        let first = MessageWrapper(OrderTracker {
            name: "first",
            order: order.clone(),
            expected: 1, // Called second
        });
        let second = MessageWrapper(OrderTracker {
            name: "second",
            order: order.clone(),
            expected: 0, // Called first
        });

        let chain = Chain(first, second);

        let headers = HeaderMap::new();
        let ctx = ResponseContext::new("test/Method", &headers);
        let mut msg = TestMessage::default();

        chain.intercept_response(&ctx, &mut msg).unwrap();
    }

    #[test]
    fn test_unit_base_case() {
        let chain = Chain((), HeaderWrapper(HeaderInterceptor::new("x-test", "value")));

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage::default();

        chain.intercept_request(&mut ctx, &mut msg).unwrap();
        assert_eq!(headers.get("x-test").unwrap(), "value");
    }

    #[test]
    fn test_nested_chain() {
        // Simulates multiple with_interceptor calls:
        // Chain(Chain((), first), second)
        let chain = Chain(
            Chain((), HeaderWrapper(HeaderInterceptor::new("x-first", "1"))),
            HeaderWrapper(HeaderInterceptor::new("x-second", "2")),
        );

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage::default();

        chain.intercept_request(&mut ctx, &mut msg).unwrap();

        assert_eq!(headers.get("x-first").unwrap(), "1");
        assert_eq!(headers.get("x-second").unwrap(), "2");
    }

    #[test]
    fn test_chain_stops_on_error() {
        #[derive(Clone)]
        struct ErrorInterceptor;

        impl Interceptor for ErrorInterceptor {
            fn on_request(&self, _ctx: &mut RequestContext) -> Result<(), ClientError> {
                Err(ClientError::invalid_argument("stopped"))
            }
        }

        #[derive(Clone)]
        struct PanicInterceptor;

        impl Interceptor for PanicInterceptor {
            fn on_request(&self, _ctx: &mut RequestContext) -> Result<(), ClientError> {
                panic!("Should not be called");
            }
        }

        let chain = Chain(HeaderWrapper(ErrorInterceptor), HeaderWrapper(PanicInterceptor));

        let mut headers = HeaderMap::new();
        let mut ctx = RequestContext::new("test/Method", &mut headers);
        let mut msg = TestMessage::default();

        let err = chain.intercept_request(&mut ctx, &mut msg).unwrap_err();
        assert_eq!(err.message(), Some("stopped"));
    }
}
