//! Response types for Connect client.
//!
//! This module provides the [`ConnectResponse`] type which wraps RPC responses
//! along with metadata (headers) from the server.

use http::{HeaderMap, HeaderName};
use std::ops::Deref;

/// How response metadata is represented on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResponseMetadataMode {
    /// Unary trailing metadata uses `Trailer-`-prefixed HTTP headers.
    Unary,
    /// Streaming trailing metadata is carried by the EndStream envelope.
    Streaming,
}

/// Build the metadata exposed by the client from response headers and
/// protocol trailers.
///
/// Values are copied as raw [`http::HeaderValue`]s. Initial header values are
/// appended before trailing values when both use the same normalized name.
pub(crate) fn normalize_response_metadata(
    headers: &HeaderMap,
    trailers: Option<&HeaderMap>,
    mode: ResponseMetadataMode,
) -> HeaderMap {
    let mut metadata =
        HeaderMap::with_capacity(headers.len() + trailers.map(HeaderMap::len).unwrap_or_default());

    // Copy leading metadata first. For unary responses, `Trailer-*` entries
    // are handled in a second pass so their values always follow ordinary
    // values with the same normalized name.
    for name in headers.keys() {
        if mode == ResponseMetadataMode::Unary && name.as_str().starts_with("trailer-") {
            continue;
        }
        append_header_values(&mut metadata, name.clone(), name, headers);
    }

    if mode == ResponseMetadataMode::Unary {
        for name in headers.keys() {
            let Some(suffix) = name.as_str().strip_prefix("trailer-") else {
                continue;
            };
            let Ok(normalized_name) = HeaderName::from_bytes(suffix.as_bytes()) else {
                continue;
            };
            append_header_values(&mut metadata, normalized_name, name, headers);
        }
    }

    if let Some(trailers) = trailers {
        for name in trailers.keys() {
            append_header_values(&mut metadata, name.clone(), name, trailers);
        }
    }

    metadata
}

fn append_header_values(
    target: &mut HeaderMap,
    target_name: HeaderName,
    source_name: &HeaderName,
    source: &HeaderMap,
) {
    for value in source.get_all(source_name) {
        target.append(target_name.clone(), value.clone());
    }
}

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
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (&http::header::HeaderName, &http::header::HeaderValue)> {
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

    #[test]
    fn unary_metadata_normalizes_trailer_prefix_after_leading_values() {
        let mut headers = HeaderMap::new();
        headers.append("x-shared", HeaderValue::from_static("header-1"));
        headers.append("x-shared", HeaderValue::from_static("header-2"));
        headers.append("trailer-x-shared", HeaderValue::from_static("trailer-1"));
        headers.append("trailer-x-shared", HeaderValue::from_static("trailer-2"));
        headers.append("trailer-payload-bin", HeaderValue::from_static("AQID"));

        let metadata = normalize_response_metadata(&headers, None, ResponseMetadataMode::Unary);

        let shared: Vec<_> = metadata
            .get_all("x-shared")
            .iter()
            .map(HeaderValue::as_bytes)
            .collect();
        assert_eq!(
            shared,
            vec![
                b"header-1".as_slice(),
                b"header-2".as_slice(),
                b"trailer-1".as_slice(),
                b"trailer-2".as_slice(),
            ]
        );
        assert!(!metadata.contains_key("trailer-x-shared"));
        assert_eq!(metadata["payload-bin"].as_bytes(), b"AQID");
    }

    #[test]
    fn streaming_metadata_keeps_trailer_prefix_and_appends_end_stream_values() {
        let mut headers = HeaderMap::new();
        headers.append("x-shared", HeaderValue::from_static("header"));
        headers.append("trailer-x-shared", HeaderValue::from_static("literal"));
        let mut trailers = HeaderMap::new();
        trailers.append("x-shared", HeaderValue::from_static("end-stream-1"));
        trailers.append("x-shared", HeaderValue::from_static("end-stream-2"));

        let metadata =
            normalize_response_metadata(&headers, Some(&trailers), ResponseMetadataMode::Streaming);

        let shared: Vec<_> = metadata
            .get_all("x-shared")
            .iter()
            .map(HeaderValue::as_bytes)
            .collect();
        assert_eq!(
            shared,
            vec![
                b"header".as_slice(),
                b"end-stream-1".as_slice(),
                b"end-stream-2".as_slice(),
            ]
        );
        assert_eq!(metadata["trailer-x-shared"], "literal");
    }
}
