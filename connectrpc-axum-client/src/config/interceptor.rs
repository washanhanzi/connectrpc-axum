//! Interceptors for Connect RPC client.
//!
//! Interceptors allow you to add cross-cutting logic to RPC calls, such as:
//! - Adding authentication headers
//! - Logging and metrics
//! - Retry logic
//! - Request/response transformation
//!
//! # Example
//!
//! ```ignore
//! use connectrpc_axum_client::{ConnectClient, HeaderInterceptor};
//!
//! // Create an interceptor that adds an auth header
//! let auth_interceptor = HeaderInterceptor::new("authorization", "Bearer token123");
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .with_interceptor(auth_interceptor)
//!     .build()?;
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http::HeaderMap;

use crate::ClientError;

/// Type alias for a boxed future returning a result.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A unary RPC request with headers and body.
///
/// This is a type-erased request that interceptors can modify before
/// it's sent to the server.
#[derive(Debug, Clone)]
pub struct UnaryRequest {
    /// The procedure being called (e.g., "package.Service/Method").
    pub procedure: String,
    /// HTTP headers for the request.
    pub headers: HeaderMap,
    /// Request body (encoded message).
    pub body: Bytes,
}

impl UnaryRequest {
    /// Create a new unary request.
    pub fn new(procedure: impl Into<String>, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            procedure: procedure.into(),
            headers,
            body,
        }
    }

    /// Get a mutable reference to the headers.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }
}

/// The type of streaming RPC call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// Server streaming: client sends one request, server sends multiple responses.
    ServerStream,
    /// Client streaming: client sends multiple requests, server sends one response.
    ClientStream,
    /// Bidirectional streaming: both client and server send multiple messages.
    BidiStream,
}

impl std::fmt::Display for StreamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamType::ServerStream => write!(f, "server_stream"),
            StreamType::ClientStream => write!(f, "client_stream"),
            StreamType::BidiStream => write!(f, "bidi_stream"),
        }
    }
}

/// A streaming RPC request context.
///
/// Provides request context for streaming interceptors, similar to how
/// [`UnaryRequest`] provides context for unary interceptors.
#[derive(Debug)]
pub struct StreamingRequest<'a> {
    /// The procedure being called (e.g., "package.Service/Method").
    pub procedure: &'a str,
    /// The type of streaming call.
    pub stream_type: StreamType,
    /// HTTP headers for the request (mutable).
    pub headers: &'a mut HeaderMap,
}

impl<'a> StreamingRequest<'a> {
    /// Create a new streaming request context.
    pub fn new(procedure: &'a str, stream_type: StreamType, headers: &'a mut HeaderMap) -> Self {
        Self {
            procedure,
            stream_type,
            headers,
        }
    }
}

/// A unary RPC response with headers and body.
///
/// This is a type-erased response that interceptors can inspect or modify.
#[derive(Debug, Clone)]
pub struct UnaryResponse {
    /// HTTP headers from the response.
    pub headers: HeaderMap,
    /// Response body (encoded message).
    pub body: Bytes,
}

impl UnaryResponse {
    /// Create a new unary response.
    pub fn new(headers: HeaderMap, body: Bytes) -> Self {
        Self { headers, body }
    }

    /// Get a mutable reference to the headers.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }
}

/// The signature of a unary RPC call.
///
/// Interceptors wrap this function to add logic before and after the call.
pub type UnaryFunc =
    Arc<dyn Fn(UnaryRequest) -> BoxFuture<'static, Result<UnaryResponse, ClientError>> + Send + Sync>;

/// The "next" function in the interceptor chain.
///
/// Call this to proceed to the next interceptor or the actual RPC call.
#[derive(Clone)]
pub struct UnaryNext {
    inner: UnaryFunc,
}

impl UnaryNext {
    /// Create a new UnaryNext wrapping a function.
    pub(crate) fn new(inner: UnaryFunc) -> Self {
        Self { inner }
    }

    /// Call the next interceptor or the actual RPC.
    pub async fn call(self, request: UnaryRequest) -> Result<UnaryResponse, ClientError> {
        (self.inner)(request).await
    }
}

/// An interceptor that can wrap unary and streaming RPC calls.
///
/// This is the main interceptor trait. Implement this to create custom
/// interceptors that can handle all RPC types.
///
/// For simpler use cases, use [`HeaderInterceptor`] which just adds headers.
pub trait Interceptor: Send + Sync {
    /// Wrap a unary RPC call.
    ///
    /// The default implementation passes through to the next function unchanged.
    fn wrap_unary(&self, next: UnaryFunc) -> UnaryFunc {
        next
    }

    /// Wrap a streaming client call.
    ///
    /// The default implementation passes through unchanged.
    /// Streaming interceptors can modify headers and access request context
    /// (procedure name and stream type) before the stream starts.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use connectrpc_axum_client::{Interceptor, StreamingRequest, StreamType};
    ///
    /// struct LoggingInterceptor;
    ///
    /// impl Interceptor for LoggingInterceptor {
    ///     fn wrap_streaming(&self, request: &mut StreamingRequest<'_>) {
    ///         println!("Streaming call to {} ({})", request.procedure, request.stream_type);
    ///         request.headers.insert("x-logged", "true".parse().unwrap());
    ///     }
    /// }
    /// ```
    fn wrap_streaming(&self, request: &mut StreamingRequest<'_>) {
        let _ = request;
    }
}

/// A chain of interceptors that are applied in order.
#[derive(Clone)]
pub struct InterceptorChain {
    interceptors: Vec<Arc<dyn Interceptor>>,
}

impl std::fmt::Debug for InterceptorChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterceptorChain")
            .field("count", &self.interceptors.len())
            .finish()
    }
}

impl InterceptorChain {
    /// Create a new empty interceptor chain.
    pub fn new() -> Self {
        Self {
            interceptors: Vec::new(),
        }
    }

    /// Add an interceptor to the chain.
    pub fn push(&mut self, interceptor: Arc<dyn Interceptor>) {
        self.interceptors.push(interceptor);
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.interceptors.is_empty()
    }

    /// Get the number of interceptors in the chain.
    pub fn len(&self) -> usize {
        self.interceptors.len()
    }

    /// Wrap a unary function with all interceptors in the chain.
    ///
    /// Interceptors are applied in reverse order so that the first interceptor
    /// added is the first to process the request.
    pub fn wrap_unary(&self, next: UnaryFunc) -> UnaryFunc {
        let mut wrapped = next;
        // Apply in reverse order so first interceptor acts first
        for interceptor in self.interceptors.iter().rev() {
            wrapped = interceptor.wrap_unary(wrapped);
        }
        wrapped
    }

    /// Apply all interceptors' streaming modifications.
    ///
    /// This gives each interceptor access to the procedure, stream type, and headers.
    pub fn apply_streaming(&self, request: &mut StreamingRequest<'_>) {
        for interceptor in &self.interceptors {
            interceptor.wrap_streaming(request);
        }
    }
}

impl Default for InterceptorChain {
    fn default() -> Self {
        Self::new()
    }
}

/// A simple interceptor that adds headers to all requests.
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
#[derive(Clone)]
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
    fn wrap_unary(&self, next: UnaryFunc) -> UnaryFunc {
        let name = self.name.clone();
        let value = self.value.clone();
        Arc::new(move |mut request: UnaryRequest| {
            request.headers.insert(name.clone(), value.clone());
            next(request)
        })
    }

    fn wrap_streaming(&self, request: &mut StreamingRequest<'_>) {
        request.headers.insert(self.name.clone(), self.value.clone());
    }
}

/// A function-based unary interceptor.
///
/// This allows creating interceptors from closures that can access the request
/// and response.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{FnInterceptor, UnaryRequest, UnaryNext};
///
/// let logging = FnInterceptor::unary(|req: UnaryRequest, next: UnaryNext| {
///     Box::pin(async move {
///         println!("Calling: {}", req.procedure);
///         let result = next.call(req).await;
///         println!("Call completed");
///         result
///     })
/// });
/// ```
pub struct FnInterceptor<F> {
    func: F,
}

impl<F> FnInterceptor<F>
where
    F: Fn(UnaryRequest, UnaryNext) -> BoxFuture<'static, Result<UnaryResponse, ClientError>>
        + Send
        + Sync
        + Clone
        + 'static,
{
    /// Create a new function-based unary interceptor.
    pub fn unary(func: F) -> Self {
        Self { func }
    }
}

impl<F> Interceptor for FnInterceptor<F>
where
    F: Fn(UnaryRequest, UnaryNext) -> BoxFuture<'static, Result<UnaryResponse, ClientError>>
        + Send
        + Sync
        + Clone
        + 'static,
{
    fn wrap_unary(&self, next: UnaryFunc) -> UnaryFunc {
        let func = self.func.clone();
        Arc::new(move |request: UnaryRequest| {
            let func = func.clone();
            let next = UnaryNext::new(next.clone());
            func(request, next)
        })
    }
}

impl<F> Clone for FnInterceptor<F>
where
    F: Clone,
{
    fn clone(&self) -> Self {
        Self {
            func: self.func.clone(),
        }
    }
}

/// Convenience type alias for creating unary interceptor functions.
pub type UnaryInterceptorFunc<F> = FnInterceptor<F>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_interceptor() {
        let interceptor = HeaderInterceptor::new("x-custom-header", "test-value");
        let mut headers = HeaderMap::new();
        let mut request = StreamingRequest::new("test/Method", StreamType::ServerStream, &mut headers);
        interceptor.wrap_streaming(&mut request);
        assert_eq!(headers.get("x-custom-header").unwrap(), "test-value");
    }

    #[test]
    fn test_interceptor_chain_empty() {
        let chain = InterceptorChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn test_interceptor_chain_push() {
        let mut chain = InterceptorChain::new();
        let interceptor = HeaderInterceptor::new("x-test", "value");
        chain.push(Arc::new(interceptor));
        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 1);
    }

    #[tokio::test]
    async fn test_header_interceptor_unary() {
        let interceptor = HeaderInterceptor::new("x-auth", "bearer-token");

        // Create a mock "next" function that captures the request
        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_clone = captured.clone();

        let next: UnaryFunc = Arc::new(move |req: UnaryRequest| {
            let captured = captured_clone.clone();
            Box::pin(async move {
                *captured.lock().unwrap() = Some(req.headers.clone());
                Ok(UnaryResponse::new(HeaderMap::new(), Bytes::new()))
            })
        });

        let wrapped = interceptor.wrap_unary(next);
        let request = UnaryRequest::new("test/Method", HeaderMap::new(), Bytes::new());
        let _ = wrapped(request).await;

        let captured_headers = captured.lock().unwrap().take().unwrap();
        assert_eq!(captured_headers.get("x-auth").unwrap(), "bearer-token");
    }

    #[tokio::test]
    async fn test_fn_interceptor() {
        let interceptor = FnInterceptor::unary(|mut req, next| {
            Box::pin(async move {
                req.headers
                    .insert("x-modified", "true".parse().unwrap());
                next.call(req).await
            })
        });

        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_clone = captured.clone();

        let next: UnaryFunc = Arc::new(move |req: UnaryRequest| {
            let captured = captured_clone.clone();
            Box::pin(async move {
                *captured.lock().unwrap() = Some(req.headers.clone());
                Ok(UnaryResponse::new(HeaderMap::new(), Bytes::new()))
            })
        });

        let wrapped = interceptor.wrap_unary(next);
        let request = UnaryRequest::new("test/Method", HeaderMap::new(), Bytes::new());
        let _ = wrapped(request).await;

        let captured_headers = captured.lock().unwrap().take().unwrap();
        assert_eq!(captured_headers.get("x-modified").unwrap(), "true");
    }

    #[tokio::test]
    async fn test_interceptor_chain_order() {
        // Test that interceptors are applied in the correct order
        // First interceptor should see the request first
        let mut chain = InterceptorChain::new();

        let interceptor1 = HeaderInterceptor::new("x-first", "1");
        let interceptor2 = HeaderInterceptor::new("x-second", "2");

        chain.push(Arc::new(interceptor1));
        chain.push(Arc::new(interceptor2));

        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_clone = captured.clone();

        let next: UnaryFunc = Arc::new(move |req: UnaryRequest| {
            let captured = captured_clone.clone();
            Box::pin(async move {
                *captured.lock().unwrap() = Some(req.headers.clone());
                Ok(UnaryResponse::new(HeaderMap::new(), Bytes::new()))
            })
        });

        let wrapped = chain.wrap_unary(next);
        let request = UnaryRequest::new("test/Method", HeaderMap::new(), Bytes::new());
        let _ = wrapped(request).await;

        let captured_headers = captured.lock().unwrap().take().unwrap();
        // Both headers should be present
        assert_eq!(captured_headers.get("x-first").unwrap(), "1");
        assert_eq!(captured_headers.get("x-second").unwrap(), "2");
    }
}
