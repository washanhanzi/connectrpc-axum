//! Intercepting stream wrappers for message-level interception.
//!
//! This module provides stream adapters that call interceptor methods
//! for each message in a streaming RPC.

use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use http::HeaderMap;
use prost::Message;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::{InterceptorInternal, StreamContext, StreamType, TypedInterceptor};
use crate::ClientError;

use super::decoder::FrameDecoder;
use super::streaming::Streaming;
use super::types::Metadata;

/// A stream wrapper that intercepts incoming messages.
///
/// This wrapper calls `intercept_stream_receive` on the interceptor for each
/// message yielded by the inner stream.
pub struct InterceptingStream<S, T, I> {
    /// The underlying stream.
    inner: S,
    /// The interceptor to call for each message.
    interceptor: I,
    /// The procedure name (e.g., "package.Service/Method").
    procedure: String,
    /// The type of stream.
    stream_type: StreamType,
    /// Request headers (for context).
    request_headers: HeaderMap,
    /// Response headers (for context).
    response_headers: HeaderMap,
    /// Marker for the message type.
    _marker: PhantomData<T>,
}

impl<S, T, I> InterceptingStream<S, T, I> {
    /// Create a new intercepting stream.
    pub fn new(
        inner: S,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            _marker: PhantomData,
        }
    }

    /// Get a reference to the inner stream.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner stream.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner stream.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, T, I> Unpin for InterceptingStream<S, T, I> where S: Unpin {}

impl<S, T, I> Stream for InterceptingStream<S, T, I>
where
    S: Stream<Item = Result<T, ClientError>> + Unpin,
    T: Message + DeserializeOwned + Default + 'static,
    I: InterceptorInternal,
{
    type Item = Result<T, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(mut msg))) => {
                // Create stream context
                let ctx = StreamContext::new(
                    &this.procedure,
                    this.stream_type,
                    &this.request_headers,
                    Some(&this.response_headers),
                );

                // Call interceptor
                match this.interceptor.intercept_stream_receive(&ctx, &mut msg) {
                    Ok(()) => Poll::Ready(Some(Ok(msg))),
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// A stream adapter that intercepts outgoing messages before encoding.
///
/// This wraps an input stream and calls `intercept_stream_send` on each message
/// before yielding it to the encoder.
pub struct InterceptingSendStream<S, T, I> {
    /// The underlying message stream.
    inner: S,
    /// The interceptor to call for each message.
    interceptor: I,
    /// The procedure name.
    procedure: String,
    /// The type of stream.
    stream_type: StreamType,
    /// Request headers (for context).
    request_headers: HeaderMap,
    /// Marker for message type.
    _marker: PhantomData<T>,
}

impl<S, T, I> InterceptingSendStream<S, T, I> {
    /// Create a new intercepting send stream.
    pub fn new(
        inner: S,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            _marker: PhantomData,
        }
    }
}

impl<S, T, I> Unpin for InterceptingSendStream<S, T, I> where S: Unpin {}

impl<S, T, I> Stream for InterceptingSendStream<S, T, I>
where
    S: Stream<Item = T> + Unpin,
    T: Message + Serialize + 'static,
    I: InterceptorInternal,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(mut msg)) => {
                // Create stream context (no response headers yet for outgoing)
                let ctx = StreamContext::new(
                    &this.procedure,
                    this.stream_type,
                    &this.request_headers,
                    None,
                );

                // Call interceptor - if it fails, we need to propagate the error
                // But our Item type is T, not Result<T, E>
                // So we'll log or silently ignore errors here
                // TODO: Consider changing to Result<T, ClientError> if needed
                let _ = this.interceptor.intercept_stream_send(&ctx, &mut msg);

                Poll::Ready(Some(msg))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

// ============================================================================
// InterceptingStreaming - Wraps Streaming<FrameDecoder<...>> with interception
// ============================================================================

/// A wrapper around `Streaming` that intercepts each received message.
///
/// This provides the same interface as `Streaming` (trailers, is_finished, drain)
/// but calls `intercept_stream_receive` on each message.
pub struct InterceptingStreaming<S, T, I> {
    /// The underlying streaming wrapper.
    inner: Streaming<S>,
    /// The interceptor.
    interceptor: I,
    /// The procedure name.
    procedure: String,
    /// The type of stream.
    stream_type: StreamType,
    /// Request headers.
    request_headers: HeaderMap,
    /// Response headers.
    response_headers: HeaderMap,
    /// Marker for message type.
    _marker: PhantomData<T>,
}

impl<S, T, I> InterceptingStreaming<S, T, I> {
    /// Create a new intercepting streaming wrapper.
    pub fn new(
        inner: Streaming<S>,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            _marker: PhantomData,
        }
    }

    /// Get the inner streaming wrapper.
    ///
    /// This consumes the intercepting wrapper and returns the underlying `Streaming<S>`.
    pub fn get_inner(self) -> Streaming<S> {
        self.inner
    }
}

impl<S, T, I> InterceptingStreaming<FrameDecoder<S, T>, T, I> {
    /// Get the trailers received in the EndStream frame.
    pub fn trailers(&self) -> Option<&Metadata> {
        self.inner.trailers()
    }

    /// Take the trailers.
    pub fn take_trailers(&mut self) -> Option<Metadata> {
        self.inner.take_trailers()
    }

    /// Check if the stream has finished.
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

impl<S, T, I> InterceptingStreaming<S, T, I>
where
    S: Stream<Item = Result<T, ClientError>> + Unpin,
{
    /// Drain remaining messages.
    pub async fn drain(&mut self) -> usize {
        self.inner.drain().await
    }

    /// Drain with timeout.
    pub async fn drain_timeout(&mut self, timeout: std::time::Duration) -> Result<usize, usize> {
        self.inner.drain_timeout(timeout).await
    }
}

impl<S, T, I> Unpin for InterceptingStreaming<S, T, I> where Streaming<S>: Unpin {}

impl<S, T, I> Stream for InterceptingStreaming<S, T, I>
where
    Streaming<S>: Stream<Item = Result<T, ClientError>> + Unpin,
    T: Message + DeserializeOwned + Default + 'static,
    I: InterceptorInternal,
{
    type Item = Result<T, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(mut msg))) => {
                // Create stream context
                let ctx = StreamContext::new(
                    &this.procedure,
                    this.stream_type,
                    &this.request_headers,
                    Some(&this.response_headers),
                );

                // Call interceptor
                match this.interceptor.intercept_stream_receive(&ctx, &mut msg) {
                    Ok(()) => Poll::Ready(Some(Ok(msg))),
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

// ============================================================================
// Typed Interceptor Stream Wrappers
// ============================================================================

/// Wrapper around `Streaming` with typed receive interceptor.
///
/// This provides the same interface as `Streaming` (trailers, is_finished, drain)
/// but applies a typed interceptor to each message.
pub struct TypedReceiveStreaming<S, T> {
    /// The underlying streaming wrapper.
    inner: Streaming<S>,
    /// The typed interceptor.
    interceptor: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, T>>>,
    /// The procedure name.
    procedure: String,
    /// The type of stream.
    stream_type: StreamType,
    /// Request headers.
    request_headers: HeaderMap,
    /// Response headers.
    response_headers: HeaderMap,
}

impl<S, T> TypedReceiveStreaming<S, T> {
    /// Create a new typed receive streaming wrapper.
    pub fn new(
        inner: Streaming<S>,
        interceptor: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, T>>>,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
        }
    }
}

impl<S, T> TypedReceiveStreaming<FrameDecoder<S, T>, T> {
    /// Get the trailers received in the EndStream frame.
    pub fn trailers(&self) -> Option<&Metadata> {
        self.inner.trailers()
    }

    /// Take the trailers.
    pub fn take_trailers(&mut self) -> Option<Metadata> {
        self.inner.take_trailers()
    }

    /// Check if the stream has finished.
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

impl<S, T> TypedReceiveStreaming<S, T>
where
    S: Stream<Item = Result<T, ClientError>> + Unpin,
{
    /// Drain remaining messages.
    pub async fn drain(&mut self) -> usize {
        self.inner.drain().await
    }

    /// Drain with timeout.
    pub async fn drain_timeout(&mut self, timeout: std::time::Duration) -> Result<usize, usize> {
        self.inner.drain_timeout(timeout).await
    }
}

impl<S, T> Unpin for TypedReceiveStreaming<S, T> where Streaming<S>: Unpin {}

impl<S, T> Stream for TypedReceiveStreaming<S, T>
where
    Streaming<S>: Stream<Item = Result<T, ClientError>> + Unpin,
    T: 'static,
{
    type Item = Result<T, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(mut msg))) => {
                // If there's an interceptor, call it
                if let Some(ref interceptor) = this.interceptor {
                    let ctx = StreamContext::new(
                        &this.procedure,
                        this.stream_type,
                        &this.request_headers,
                        Some(&this.response_headers),
                    );

                    match interceptor.intercept(&ctx, &mut msg) {
                        Ok(()) => Poll::Ready(Some(Ok(msg))),
                        Err(e) => Poll::Ready(Some(Err(e))),
                    }
                } else {
                    Poll::Ready(Some(Ok(msg)))
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
