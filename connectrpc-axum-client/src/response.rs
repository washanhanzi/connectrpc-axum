//! Response types for Connect client.
//!
//! This module provides the [`ConnectResponse`] type which wraps RPC responses
//! along with metadata (headers) from the server.

use http::HeaderMap;
use std::ops::Deref;

/// Response wrapper for Connect RPC client calls.
///
/// Contains the response message and associated metadata (HTTP headers)
/// from the server response.
///
/// # Example
///
/// ```ignore
/// let response = client.call_unary::<Req, Res>("pkg.Service/Method", &req).await?;
///
/// // Access the response directly via Deref
/// println!("Name: {}", response.name);
///
/// // Or extract the inner value
/// let inner = response.into_inner();
///
/// // Access response metadata (headers)
/// if let Some(value) = response.metadata().get("x-custom-header") {
///     println!("Custom header: {:?}", value);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ConnectResponse<T> {
    /// The response message.
    inner: T,
    /// Response metadata (HTTP headers).
    metadata: Metadata,
}

impl<T> ConnectResponse<T> {
    /// Create a new ConnectResponse with the given value and metadata.
    pub fn new(inner: T, metadata: Metadata) -> Self {
        Self { inner, metadata }
    }

    /// Extract the inner value, discarding metadata.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Get a reference to the response metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Get a mutable reference to the response metadata.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Transform the inner value, preserving metadata.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response: ConnectResponse<User> = client.call_unary(...).await?;
    /// let name_response: ConnectResponse<String> = response.map(|user| user.name);
    /// ```
    pub fn map<U, F>(self, f: F) -> ConnectResponse<U>
    where
        F: FnOnce(T) -> U,
    {
        ConnectResponse {
            inner: f(self.inner),
            metadata: self.metadata,
        }
    }

    /// Get a reference to the inner value.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the inner value.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Decompose into inner value and metadata.
    pub fn into_parts(self) -> (T, Metadata) {
        (self.inner, self.metadata)
    }
}

impl<T> Deref for ConnectResponse<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> AsRef<T> for ConnectResponse<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

/// Response metadata wrapper around HTTP headers.
///
/// Provides convenient access to response headers returned by the server.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    headers: HeaderMap,
}

impl Metadata {
    /// Create new metadata from HTTP headers.
    pub fn new(headers: HeaderMap) -> Self {
        Self { headers }
    }

    /// Create empty metadata.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Get a header value by name.
    ///
    /// Returns `None` if the header is not present or cannot be converted to a string.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(|v| v.to_str().ok())
    }

    /// Get a header value as bytes.
    pub fn get_bytes(&self, key: &str) -> Option<&[u8]> {
        self.headers.get(key).map(|v| v.as_bytes())
    }

    /// Check if a header exists.
    pub fn contains(&self, key: &str) -> bool {
        self.headers.contains_key(key)
    }

    /// Get all values for a header (for headers that appear multiple times).
    pub fn get_all(&self, key: &str) -> impl Iterator<Item = &str> {
        self.headers
            .get_all(key)
            .iter()
            .filter_map(|v| v.to_str().ok())
    }

    /// Get the underlying HeaderMap.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the underlying HeaderMap.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Consume self and return the underlying HeaderMap.
    pub fn into_headers(self) -> HeaderMap {
        self.headers
    }

    /// Get an iterator over all header names and values.
    pub fn iter(&self) -> impl Iterator<Item = (&http::header::HeaderName, &http::header::HeaderValue)> {
        self.headers.iter()
    }

    /// Returns true if there are no headers.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// Returns the number of headers.
    pub fn len(&self) -> usize {
        self.headers.len()
    }
}

impl From<HeaderMap> for Metadata {
    fn from(headers: HeaderMap) -> Self {
        Self::new(headers)
    }
}

impl From<Metadata> for HeaderMap {
    fn from(metadata: Metadata) -> Self {
        metadata.headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::HeaderValue;

    #[test]
    fn test_connect_response_new() {
        let metadata = Metadata::empty();
        let response = ConnectResponse::new(42, metadata);
        assert_eq!(*response, 42);
    }

    #[test]
    fn test_connect_response_into_inner() {
        let response = ConnectResponse::new("hello".to_string(), Metadata::empty());
        let inner = response.into_inner();
        assert_eq!(inner, "hello");
    }

    #[test]
    fn test_connect_response_map() {
        let response = ConnectResponse::new(5, Metadata::empty());
        let mapped = response.map(|x| x * 2);
        assert_eq!(*mapped, 10);
    }

    #[test]
    fn test_connect_response_deref() {
        let response = ConnectResponse::new(vec![1, 2, 3], Metadata::empty());
        assert_eq!(response.len(), 3); // Using Vec's len() via Deref
    }

    #[test]
    fn test_metadata_get() {
        let mut headers = HeaderMap::new();
        headers.insert("x-custom", HeaderValue::from_static("value"));
        let metadata = Metadata::new(headers);

        assert_eq!(metadata.get("x-custom"), Some("value"));
        assert_eq!(metadata.get("missing"), None);
    }

    #[test]
    fn test_metadata_contains() {
        let mut headers = HeaderMap::new();
        headers.insert("x-present", HeaderValue::from_static("yes"));
        let metadata = Metadata::new(headers);

        assert!(metadata.contains("x-present"));
        assert!(!metadata.contains("x-absent"));
    }

    #[test]
    fn test_connect_response_into_parts() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", HeaderValue::from_static("test-value"));
        let metadata = Metadata::new(headers);
        let response = ConnectResponse::new(42, metadata);

        let (inner, metadata) = response.into_parts();
        assert_eq!(inner, 42);
        assert_eq!(metadata.get("x-test"), Some("test-value"));
    }
}
