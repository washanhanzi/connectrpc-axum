//! Intercepting stream wrappers for message-level interception.
//!
//! This module provides stream adapters that call interceptor methods
//! for each message in a streaming RPC.

use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use futures::Stream;
use http::HeaderMap;
use prost::Message;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ClientError;
use crate::config::{InterceptorInternal, StreamContext, StreamType, TypedInterceptor};

use super::decoder::FrameDecoder;
use super::streaming::Streaming;
use super::types::Metadata;

#[doc(hidden)]
pub struct RequestStreamError {
    inner: Mutex<RequestStreamErrorInner>,
}

#[derive(Default)]
struct RequestStreamErrorInner {
    error: Option<ClientError>,
    waker: Option<Waker>,
}

impl RequestStreamError {
    #[doc(hidden)]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(RequestStreamErrorInner::default()),
        })
    }

    #[doc(hidden)]
    pub fn store(&self, error: ClientError) {
        let waker = {
            let mut inner = self.inner.lock().unwrap();
            if inner.error.is_none() {
                inner.error = Some(error);
            }
            inner.waker.take()
        };

        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn take(&self) -> Option<ClientError> {
        self.inner.lock().unwrap().error.take()
    }

    fn take_or_register(&self, cx: &Context<'_>) -> Option<ClientError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(error) = inner.error.take() {
            return Some(error);
        }

        match inner.waker.as_ref() {
            Some(waker) if waker.will_wake(cx.waker()) => {}
            _ => inner.waker = Some(cx.waker().clone()),
        }

        None
    }
}

pub(crate) fn take_request_stream_error(
    request_error: &Arc<RequestStreamError>,
) -> Option<ClientError> {
    request_error.take()
}

fn take_optional_request_stream_error(
    request_error: &Option<Arc<RequestStreamError>>,
) -> Option<ClientError> {
    request_error
        .as_ref()
        .and_then(|request_error| request_error.take())
}

fn take_or_register_optional_request_stream_error(
    request_error: &Option<Arc<RequestStreamError>>,
    cx: &Context<'_>,
) -> Option<ClientError> {
    request_error
        .as_ref()
        .and_then(|request_error| request_error.take_or_register(cx))
}

/// A stream wrapper that intercepts incoming messages.
///
/// This wrapper calls `intercept_stream_receive` on the interceptor for each
/// message yielded by the inner stream.
#[deprecated(
    note = "use `InterceptingStreaming` instead, which also exposes trailers and drain helpers"
)]
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

#[allow(deprecated)]
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

#[allow(deprecated)]
impl<S, T, I> Unpin for InterceptingStream<S, T, I> where S: Unpin {}

#[allow(deprecated)]
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
    /// Shared request stream error storage.
    request_error: Option<Arc<RequestStreamError>>,
    /// Whether an outbound interceptor error has aborted this stream.
    aborted: bool,
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
        Self::new_inner(
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            None,
        )
    }

    /// Create a new intercepting send stream that records request stream errors.
    pub fn with_request_error_capture(
        inner: S,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        request_error: Arc<RequestStreamError>,
    ) -> Self {
        Self::new_inner(
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            Some(request_error),
        )
    }

    fn new_inner(
        inner: S,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        request_error: Option<Arc<RequestStreamError>>,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            request_error,
            aborted: false,
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
    type Item = Result<T, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.aborted {
            return Poll::Ready(None);
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(mut msg)) => {
                // Create stream context (no response headers yet for outgoing)
                let ctx = StreamContext::new(
                    &this.procedure,
                    this.stream_type,
                    &this.request_headers,
                    None,
                );

                match this.interceptor.intercept_stream_send(&ctx, &mut msg) {
                    Ok(()) => Poll::Ready(Some(Ok(msg))),
                    Err(e) => {
                        this.aborted = true;
                        if let Some(ref request_error) = this.request_error {
                            request_error.store(e.clone());
                        }
                        Poll::Ready(Some(Err(e)))
                    }
                }
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
    /// Shared request stream error storage.
    request_error: Option<Arc<RequestStreamError>>,
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
        Self::new_inner(
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            None,
        )
    }

    /// Create a new intercepting streaming wrapper that can yield request stream errors.
    ///
    /// Captured send errors are yielded before buffered received messages.
    pub fn with_request_error_capture(
        inner: Streaming<S>,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
        request_error: Arc<RequestStreamError>,
    ) -> Self {
        Self::new_inner(
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            Some(request_error),
        )
    }

    fn new_inner(
        inner: Streaming<S>,
        interceptor: I,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
        request_error: Option<Arc<RequestStreamError>>,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            request_error,
            _marker: PhantomData,
        }
    }

    /// Get the inner streaming wrapper.
    ///
    /// This consumes the intercepting wrapper and returns the underlying `Streaming<S>`.
    pub fn into_inner(self) -> Streaming<S> {
        self.inner
    }

    /// Get the request headers sent for this call.
    #[doc(hidden)]
    pub fn request_headers(&self) -> &HeaderMap {
        &self.request_headers
    }

    /// Get the inner streaming wrapper.
    #[deprecated(note = "renamed to `into_inner`")]
    pub fn get_inner(self) -> Streaming<S> {
        self.into_inner()
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

        if let Some(e) = take_optional_request_stream_error(&this.request_error) {
            return Poll::Ready(Some(Err(e)));
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(mut msg))) => {
                if let Some(e) = take_optional_request_stream_error(&this.request_error) {
                    return Poll::Ready(Some(Err(e)));
                }

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
            Poll::Ready(Some(Err(e))) => {
                if let Some(request_error) = take_optional_request_stream_error(&this.request_error)
                {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(
                        procedure = this.procedure.as_str(),
                        receive_error = %&e,
                        request_stream_error = %&request_error,
                        "receive stream error suppressed by request stream error"
                    );

                    Poll::Ready(Some(Err(request_error)))
                } else {
                    Poll::Ready(Some(Err(e)))
                }
            }
            Poll::Ready(None) => {
                // Only an already-captured send error can replace stream completion. If a bidi
                // request stream failure is discovered after this returns None, there is no receive poll left
                // to observe it.
                if let Some(e) = take_optional_request_stream_error(&this.request_error) {
                    Poll::Ready(Some(Err(e)))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => {
                if let Some(e) =
                    take_or_register_optional_request_stream_error(&this.request_error, cx)
                {
                    Poll::Ready(Some(Err(e)))
                } else {
                    Poll::Pending
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

// ============================================================================
// Typed Interceptor Stream Wrappers
// ============================================================================

/// A stream adapter that applies a typed `on_send` interceptor to outgoing messages.
///
/// This is the typed counterpart of [`InterceptingSendStream`], used by generated
/// clients. On the first interceptor error the stream records the error in the
/// shared [`RequestStreamError`] slot, yields the error once, and then ends.
/// The yielded error aborts the HTTP request body (the server sees a broken
/// request, not a clean end-of-stream), and the recorded error takes precedence
/// over any transport or server error observed afterwards.
pub struct TypedSendStream<S, T> {
    /// The underlying message stream.
    inner: S,
    /// The typed interceptor.
    interceptor: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, T>>>,
    /// The procedure name.
    procedure: String,
    /// The type of stream.
    stream_type: StreamType,
    /// Request headers (for context).
    request_headers: HeaderMap,
    /// Shared request stream error storage.
    request_error: Option<Arc<RequestStreamError>>,
    /// Whether an outbound interceptor error has aborted this stream.
    aborted: bool,
}

impl<S, T> TypedSendStream<S, T> {
    /// Create a new typed send stream that records request stream errors.
    pub fn with_request_error_capture(
        inner: S,
        interceptor: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, T>>>,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        request_error: Arc<RequestStreamError>,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            request_error: Some(request_error),
            aborted: false,
        }
    }
}

impl<S, T> Unpin for TypedSendStream<S, T> where S: Unpin {}

impl<S, T> Stream for TypedSendStream<S, T>
where
    S: Stream<Item = T> + Unpin,
    T: 'static,
{
    type Item = Result<T, ClientError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.aborted {
            return Poll::Ready(None);
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(mut msg)) => {
                if let Some(ref interceptor) = this.interceptor {
                    // Create stream context (no response headers yet for outgoing)
                    let ctx = StreamContext::new(
                        &this.procedure,
                        this.stream_type,
                        &this.request_headers,
                        None,
                    );

                    match interceptor.intercept(&ctx, &mut msg) {
                        Ok(()) => Poll::Ready(Some(Ok(msg))),
                        Err(e) => {
                            this.aborted = true;
                            if let Some(ref request_error) = this.request_error {
                                request_error.store(e.clone());
                            }
                            Poll::Ready(Some(Err(e)))
                        }
                    }
                } else {
                    Poll::Ready(Some(Ok(msg)))
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

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
    /// Shared request stream error storage.
    request_error: Option<Arc<RequestStreamError>>,
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
        Self::new_inner(
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            None,
        )
    }

    /// Create a new typed receive stream that can yield request stream errors.
    ///
    /// Captured send errors are yielded before buffered received messages.
    pub fn with_request_error_capture(
        inner: Streaming<S>,
        interceptor: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, T>>>,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
        request_error: Arc<RequestStreamError>,
    ) -> Self {
        Self::new_inner(
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            Some(request_error),
        )
    }

    fn new_inner(
        inner: Streaming<S>,
        interceptor: Option<Arc<dyn for<'a> TypedInterceptor<StreamContext<'a>, T>>>,
        procedure: String,
        stream_type: StreamType,
        request_headers: HeaderMap,
        response_headers: HeaderMap,
        request_error: Option<Arc<RequestStreamError>>,
    ) -> Self {
        Self {
            inner,
            interceptor,
            procedure,
            stream_type,
            request_headers,
            response_headers,
            request_error,
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

        if let Some(e) = take_optional_request_stream_error(&this.request_error) {
            return Poll::Ready(Some(Err(e)));
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(mut msg))) => {
                if let Some(e) = take_optional_request_stream_error(&this.request_error) {
                    return Poll::Ready(Some(Err(e)));
                }

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
            Poll::Ready(Some(Err(e))) => {
                if let Some(request_error) = take_optional_request_stream_error(&this.request_error)
                {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(
                        procedure = this.procedure.as_str(),
                        receive_error = %&e,
                        request_stream_error = %&request_error,
                        "receive stream error suppressed by request stream error"
                    );

                    Poll::Ready(Some(Err(request_error)))
                } else {
                    Poll::Ready(Some(Err(e)))
                }
            }
            Poll::Ready(None) => {
                // Only an already-captured send error can replace stream completion. If a bidi
                // request stream failure is discovered after this returns None, there is no receive poll left
                // to observe it.
                if let Some(e) = take_optional_request_stream_error(&this.request_error) {
                    Poll::Ready(Some(Err(e)))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => {
                if let Some(e) =
                    take_or_register_optional_request_stream_error(&this.request_error, cx)
                {
                    Poll::Ready(Some(Err(e)))
                } else {
                    Poll::Pending
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MessageInterceptor, MessageWrapper};
    use futures::StreamExt;
    use futures::stream;
    use futures::task::{ArcWake, waker_ref};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Debug, PartialEq, Default)]
    struct TestMessage {
        value: String,
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

    #[derive(Clone)]
    struct FailingSendInterceptor;

    #[derive(Default)]
    struct WakeCounter {
        count: AtomicUsize,
    }

    impl ArcWake for WakeCounter {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            arc_self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl MessageInterceptor for FailingSendInterceptor {
        fn on_stream_send<Req>(
            &self,
            _ctx: &StreamContext,
            _request: &mut Req,
        ) -> Result<(), ClientError>
        where
            Req: Message + Serialize + 'static,
        {
            Err(ClientError::invalid_argument("send blocked"))
        }
    }

    #[tokio::test]
    async fn test_send_interceptor_error_is_returned_and_recorded() {
        let request_error = RequestStreamError::new();
        let messages = stream::iter(vec![
            TestMessage {
                value: "one".to_string(),
            },
            TestMessage {
                value: "two".to_string(),
            },
        ]);
        let mut stream = InterceptingSendStream::with_request_error_capture(
            messages,
            MessageWrapper(FailingSendInterceptor),
            "test.Service/ClientStream".to_string(),
            StreamType::ClientStream,
            HeaderMap::new(),
            request_error.clone(),
        );

        let err = stream.next().await.unwrap().unwrap_err();
        assert_eq!(err.message(), Some("send blocked"));

        let recorded = take_request_stream_error(&request_error).unwrap();
        assert_eq!(recorded.message(), Some("send blocked"));

        assert!(stream.next().await.is_none());
    }

    #[test]
    fn test_send_interceptor_error_wakes_pending_receive_stream() {
        let request_error = RequestStreamError::new();
        let streaming = Streaming::new(stream::pending::<Result<TestMessage, ClientError>>());
        let mut stream = TypedReceiveStreaming::with_request_error_capture(
            streaming,
            None,
            "test.Service/BidiStream".to_string(),
            StreamType::BidiStream,
            HeaderMap::new(),
            HeaderMap::new(),
            request_error.clone(),
        );
        let wake_counter = Arc::new(WakeCounter::default());
        let waker = waker_ref(&wake_counter);
        let mut cx = Context::from_waker(&waker);

        assert!(matches!(
            Pin::new(&mut stream).poll_next(&mut cx),
            Poll::Pending
        ));

        request_error.store(ClientError::invalid_argument("send blocked"));

        assert_eq!(wake_counter.count.load(Ordering::SeqCst), 1);
        match Pin::new(&mut stream).poll_next(&mut cx) {
            Poll::Ready(Some(Err(e))) => {
                assert_eq!(e.message(), Some("send blocked"));
            }
            other => panic!("expected request stream error, got {other:?}"),
        }
    }
}
