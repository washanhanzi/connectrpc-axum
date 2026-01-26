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
//! use connectrpc_axum_client::{HeaderInterceptor, Interceptor, InterceptContext};
//!
//! // Simple header interceptor
//! let auth = HeaderInterceptor::new("authorization", "Bearer token123");
//!
//! // Custom interceptor with closure
//! let logging = Interceptor::new(|ctx: &mut InterceptContext<'_>| {
//!     println!("Calling: {}", ctx.procedure);
//!     Ok(())
//! });
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .with_interceptor(auth)
//!     .with_interceptor(logging)
//!     .build()?;
//! ```

use http::HeaderMap;

use crate::ClientError;

// ============================================================================
// Intercept Trait
// ============================================================================

/// Context for an RPC call that interceptors can inspect and modify.
#[derive(Debug)]
pub struct InterceptContext<'a> {
    /// The procedure being called (e.g., "package.Service/Method").
    pub procedure: &'a str,
    /// HTTP headers for the request (mutable).
    pub headers: &'a mut HeaderMap,
}

impl<'a> InterceptContext<'a> {
    /// Create a new intercept context.
    pub fn new(procedure: &'a str, headers: &'a mut HeaderMap) -> Self {
        Self { procedure, headers }
    }
}

/// Trait for intercepting RPC calls.
///
/// Implementations can modify headers, log calls, or return errors to abort.
///
/// # Generic Composition
///
/// Interceptors can be composed at compile time using the [`Chain`] combinator,
/// which eliminates dynamic dispatch overhead. The unit type `()` serves as the
/// base case (no-op interceptor).
///
/// ```ignore
/// use connectrpc_axum_client::{HeaderInterceptor, Chain};
///
/// // Compose interceptors at compile time
/// let auth = HeaderInterceptor::new("authorization", "Bearer token");
/// let trace = HeaderInterceptor::new("x-trace-id", "123");
/// let chain: Chain<HeaderInterceptor, HeaderInterceptor> = Chain(auth, trace);
/// ```
pub trait Intercept: Send + Sync {
    /// Called before the RPC request is sent.
    ///
    /// Interceptors can modify headers or return an error to abort the call.
    fn before_request(&self, ctx: &mut InterceptContext<'_>) -> Result<(), ClientError> {
        let _ = ctx;
        Ok(())
    }

    /// Called after the RPC response is received.
    ///
    /// Interceptors can inspect response headers.
    fn after_response(&self, headers: &HeaderMap) {
        let _ = headers;
    }
}

// ============================================================================
// Base Case: Unit Type
// ============================================================================

/// The unit type implements `Intercept` as a no-op, serving as the base case
/// for generic interceptor chains.
impl Intercept for () {
    #[inline]
    fn before_request(&self, _ctx: &mut InterceptContext<'_>) -> Result<(), ClientError> {
        Ok(())
    }

    #[inline]
    fn after_response(&self, _headers: &HeaderMap) {}
}

// ============================================================================
// Chain Combinator
// ============================================================================

/// A compile-time chain of two interceptors.
///
/// `Chain<A, B>` applies interceptor `A` first, then `B` for requests.
/// For responses, they are applied in reverse order (`B` then `A`).
///
/// This enables zero-cost interceptor composition without dynamic dispatch.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{HeaderInterceptor, Chain, Intercept, InterceptContext};
///
/// let auth = HeaderInterceptor::new("authorization", "Bearer token");
/// let trace = HeaderInterceptor::new("x-trace-id", "abc123");
///
/// // Chain them together
/// let interceptors = Chain(auth, trace);
///
/// // Use with ClientBuilder (this is done automatically)
/// let client = ConnectClient::builder("http://localhost:3000")
///     .with_interceptor(HeaderInterceptor::new("authorization", "Bearer token"))
///     .with_interceptor(HeaderInterceptor::new("x-trace-id", "abc123"))
///     .build()?;
/// ```
#[derive(Clone, Debug)]
pub struct Chain<A, B>(pub A, pub B);

impl<A, B> Intercept for Chain<A, B>
where
    A: Intercept,
    B: Intercept,
{
    #[inline]
    fn before_request(&self, ctx: &mut InterceptContext<'_>) -> Result<(), ClientError> {
        self.0.before_request(ctx)?;
        self.1.before_request(ctx)
    }

    #[inline]
    fn after_response(&self, headers: &HeaderMap) {
        // Reverse order for responses (like middleware unwinding)
        self.1.after_response(headers);
        self.0.after_response(headers);
    }
}

// ============================================================================
// Header Interceptor
// ============================================================================

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

impl Intercept for HeaderInterceptor {
    fn before_request(&self, ctx: &mut InterceptContext<'_>) -> Result<(), ClientError> {
        ctx.headers.insert(self.name.clone(), self.value.clone());
        Ok(())
    }
}

// ============================================================================
// Closure Interceptor
// ============================================================================

/// A wrapper that adapts a closure to the `Intercept` trait.
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{Interceptor, InterceptContext};
///
/// let logging = Interceptor::new(|ctx: &mut InterceptContext<'_>| {
///     println!("Calling: {}", ctx.procedure);
///     Ok(())
/// });
/// ```
pub struct Interceptor<F> {
    before: F,
}

impl<F> Interceptor<F>
where
    F: Fn(&mut InterceptContext<'_>) -> Result<(), ClientError> + Send + Sync,
{
    /// Create a new interceptor from a closure.
    pub fn new(before: F) -> Self {
        Self { before }
    }
}

impl<F> Clone for Interceptor<F>
where
    F: Clone,
{
    fn clone(&self) -> Self {
        Self {
            before: self.before.clone(),
        }
    }
}

impl<F> std::fmt::Debug for Interceptor<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interceptor").finish()
    }
}

impl<F> Intercept for Interceptor<F>
where
    F: Fn(&mut InterceptContext<'_>) -> Result<(), ClientError> + Send + Sync,
{
    fn before_request(&self, ctx: &mut InterceptContext<'_>) -> Result<(), ClientError> {
        (self.before)(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_interceptor() {
        let interceptor = HeaderInterceptor::new("x-custom-header", "test-value");
        let mut headers = HeaderMap::new();
        let mut ctx = InterceptContext::new("test/Method", &mut headers);

        interceptor.before_request(&mut ctx).unwrap();

        assert_eq!(headers.get("x-custom-header").unwrap(), "test-value");
    }

    #[test]
    fn test_unit_interceptor_noop() {
        // Unit type is a no-op interceptor
        let interceptor = ();
        let mut headers = HeaderMap::new();
        let mut ctx = InterceptContext::new("test/Method", &mut headers);

        interceptor.before_request(&mut ctx).unwrap();
        assert!(headers.is_empty()); // No headers added
    }

    #[test]
    fn test_chain_interceptors() {
        // Chain two interceptors together
        let first = HeaderInterceptor::new("x-first", "1");
        let second = HeaderInterceptor::new("x-second", "2");
        let chain = Chain(first, second);

        let mut headers = HeaderMap::new();
        let mut ctx = InterceptContext::new("test/Method", &mut headers);

        chain.before_request(&mut ctx).unwrap();

        assert_eq!(headers.get("x-first").unwrap(), "1");
        assert_eq!(headers.get("x-second").unwrap(), "2");
    }

    #[test]
    fn test_chain_with_unit_base() {
        // Chain with () as base (typical usage via builder)
        let chain = Chain((), HeaderInterceptor::new("x-header", "value"));

        let mut headers = HeaderMap::new();
        let mut ctx = InterceptContext::new("test/Method", &mut headers);

        chain.before_request(&mut ctx).unwrap();

        assert_eq!(headers.get("x-header").unwrap(), "value");
    }

    #[test]
    fn test_nested_chain() {
        // Nested chains (like from multiple with_interceptor calls)
        let chain = Chain(
            Chain((), HeaderInterceptor::new("x-first", "1")),
            HeaderInterceptor::new("x-second", "2"),
        );

        let mut headers = HeaderMap::new();
        let mut ctx = InterceptContext::new("test/Method", &mut headers);

        chain.before_request(&mut ctx).unwrap();

        assert_eq!(headers.get("x-first").unwrap(), "1");
        assert_eq!(headers.get("x-second").unwrap(), "2");
    }

    #[test]
    fn test_closure_interceptor() {
        let interceptor = Interceptor::new(|ctx: &mut InterceptContext<'_>| {
            ctx.headers.insert("x-custom", "value".parse().unwrap());
            Ok(())
        });

        let mut headers = HeaderMap::new();
        let mut ctx = InterceptContext::new("test/Method", &mut headers);

        interceptor.before_request(&mut ctx).unwrap();

        assert_eq!(headers.get("x-custom").unwrap(), "value");
    }
}
