//! Tonic-compatible handler wrappers and factory traits.
//!
//! This module provides wrappers for handlers that can be used with both Connect protocol
//! and tonic gRPC, along with factory traits for deferred handler boxing.

use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    handler::Handler,
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use futures::Stream;
use std::{future::Future, pin::Pin};

use crate::{
    context::{Context, validate_streaming_content_type},
    error::ConnectError,
    handler::handle_extractor_rejection,
    message::{ConnectRequest, ConnectResponse, StreamBody, Streaming},
};
use prost::Message;
use serde::de::DeserializeOwned;

use super::parts::RequestContext;

// =============== Factory trait for deferred handler boxing ===============

/// Tonic-style handler wrapper with axum extractor support
///
/// This wrapper accepts handlers following tonic-like patterns extended with
/// axum's `FromRequestParts` extractors (1-8 extractors supported).
/// The final argument must always be `ConnectRequest<Req>`.
#[derive(Clone)]
pub struct TonicCompatibleHandlerWrapper<F>(pub F);

/// Tonic-style streaming handler wrapper with axum extractor support
#[derive(Clone)]
pub struct TonicCompatibleStreamHandlerWrapper<F>(pub F);

// =============== Boxed callable and IntoFactory adapters ===============

/// Boxed callable used by generated tonic-compatible services.
///
/// Takes an `Option<RequestContext>` (for extractor support) and `ConnectRequest<Req>`.
/// The `RequestContext` provides access to HTTP request parts for `FromRequestParts` extraction.
/// When `None` is passed (middleware not applied), handlers without extractors work fine,
/// but handlers with extractors will return an error.
pub type BoxedCall<Req, Resp> = Box<
    dyn Fn(
            Option<RequestContext>,
            ConnectRequest<Req>,
        ) -> Pin<
            Box<dyn Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static>,
        > + Send
        + Sync,
>;

/// Boxed stream type for streaming responses
pub type BoxedStream<T> = Pin<Box<dyn Stream<Item = Result<T, ConnectError>> + Send + 'static>>;

/// Boxed callable for server streaming methods used by generated tonic-compatible services.
///
/// Takes an `Option<RequestContext>` (for extractor support) and `ConnectRequest<Req>`.
/// When `None` is passed, handlers without extractors work fine, but handlers with extractors
/// will return an error.
pub type BoxedStreamCall<Req, Resp> = Box<
    dyn Fn(
            Option<RequestContext>,
            ConnectRequest<Req>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            ConnectResponse<StreamBody<BoxedStream<Resp>>>,
                            ConnectError,
                        >,
                    > + Send
                    + 'static,
            >,
        > + Send
        + Sync,
>;

/// Boxed callable for client streaming methods used by generated tonic-compatible services.
///
/// Takes an `Option<RequestContext>` (for extractor support) and `ConnectRequest<Streaming<Req>>`.
/// When `None` is passed, handlers without extractors work fine, but handlers with extractors
/// will return an error.
pub type BoxedClientStreamCall<Req, Resp> = Box<
    dyn Fn(
            Option<RequestContext>,
            ConnectRequest<Streaming<Req>>,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            >,
        > + Send
        + Sync,
>;

/// Boxed callable for bidirectional streaming methods used by generated tonic-compatible services.
///
/// Takes an `Option<RequestContext>` (for extractor support) and `ConnectRequest<Streaming<Req>>`.
/// When `None` is passed, handlers without extractors work fine, but handlers with extractors
/// will return an error.
pub type BoxedBidiStreamCall<Req, Resp> = Box<
    dyn Fn(
            Option<RequestContext>,
            ConnectRequest<Streaming<Req>>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            ConnectResponse<StreamBody<BoxedStream<Resp>>>,
                            ConnectError,
                        >,
                    > + Send
                    + 'static,
            >,
        > + Send
        + Sync,
>;

/// Adapter that turns a user handler `F` into a factory that, given `&S`, yields a `BoxedCall`.
/// Keyed by the extractor tuple `T` to select the appropriate implementation.
pub trait IntoFactory<T, Req, Resp, S> {
    fn into_factory(self) -> Box<dyn Fn(&S) -> BoxedCall<Req, Resp> + Send + Sync>;
}

/// Adapter that turns a streaming user handler `F` into a factory that, given `&S`, yields a `BoxedStreamCall`.
/// Keyed by the extractor tuple `T` to select the appropriate implementation.
pub trait IntoStreamFactory<T, Req, Resp, S> {
    fn into_stream_factory(self) -> Box<dyn Fn(&S) -> BoxedStreamCall<Req, Resp> + Send + Sync>;
}

/// Adapter that turns a client streaming user handler `F` into a factory that, given `&S`, yields a `BoxedClientStreamCall`.
/// Keyed by the extractor tuple `T` to select the appropriate implementation.
pub trait IntoClientStreamFactory<T, Req, Resp, S> {
    fn into_client_stream_factory(
        self,
    ) -> Box<dyn Fn(&S) -> BoxedClientStreamCall<Req, Resp> + Send + Sync>;
}

/// Adapter that turns a bidi streaming user handler `F` into a factory that, given `&S`, yields a `BoxedBidiStreamCall`.
/// Keyed by the extractor tuple `T` to select the appropriate implementation.
pub trait IntoBidiStreamFactory<T, Req, Resp, S> {
    fn into_bidi_stream_factory(
        self,
    ) -> Box<dyn Fn(&S) -> BoxedBidiStreamCall<Req, Resp> + Send + Sync>;
}

/// Tonic-style client streaming handler wrapper with axum extractor support
#[derive(Clone)]
pub struct TonicCompatibleClientStreamHandlerWrapper<F>(pub F);

/// Tonic-style bidi streaming handler wrapper with axum extractor support
#[derive(Clone)]
pub struct TonicCompatibleBidiStreamHandlerWrapper<F>(pub F);

// =============== IntoFactory implementations ===============

// no-extractor: (ConnectRequest<Req>,)
impl<F, Fut, Req, Resp, S> IntoFactory<(ConnectRequest<Req>,), Req, Resp, S>
    for TonicCompatibleHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_factory(self) -> Box<dyn Fn(&S) -> BoxedCall<Req, Resp> + Send + Sync> {
        let f = self.0;
        Box::new(move |_state: &S| {
            let f = f.clone();
            // No extractors needed, so Option<RequestContext> is ignored
            Box::new(
                move |_ctx: Option<RequestContext>, req: ConnectRequest<Req>| {
                    let fut = f(req);
                    Box::pin(fut)
                },
            )
        })
    }
}

// =============== IntoStreamFactory implementations ===============

// no-extractor: (ConnectRequest<Req>,)
impl<F, Fut, Req, Resp, St, S> IntoStreamFactory<(ConnectRequest<Req>,), Req, Resp, S>
    for TonicCompatibleStreamHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    S: Clone + Send + Sync + 'static,
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    fn into_stream_factory(self) -> Box<dyn Fn(&S) -> BoxedStreamCall<Req, Resp> + Send + Sync> {
        let f = self.0;
        Box::new(move |_state: &S| {
            let f = f.clone();
            // No extractors needed, so Option<RequestContext> is ignored
            Box::new(
                move |_ctx: Option<RequestContext>, req: ConnectRequest<Req>| {
                    let fut = f(req);
                    Box::pin(async move {
                        fut.await.map(|response| {
                            let stream = response.into_inner().into_inner();
                            let boxed_stream: BoxedStream<Resp> = Box::pin(stream);
                            ConnectResponse::new(StreamBody::new(boxed_stream))
                        })
                    })
                },
            )
        })
    }
}

// =============== IntoClientStreamFactory implementations ===============

// no-extractor: (ConnectRequest<Streaming<Req>>,)
impl<F, Fut, Req, Resp, S> IntoClientStreamFactory<(ConnectRequest<Streaming<Req>>,), Req, Resp, S>
    for TonicCompatibleClientStreamHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    S: Clone + Send + Sync + 'static,
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    fn into_client_stream_factory(
        self,
    ) -> Box<dyn Fn(&S) -> BoxedClientStreamCall<Req, Resp> + Send + Sync> {
        let f = self.0;
        Box::new(move |_state: &S| {
            let f = f.clone();
            Box::new(
                move |_ctx: Option<RequestContext>, req: ConnectRequest<Streaming<Req>>| {
                    let fut = f(req);
                    Box::pin(fut)
                },
            )
        })
    }
}

// =============== IntoBidiStreamFactory implementations ===============

// no-extractor: (ConnectRequest<Streaming<Req>>,)
impl<F, Fut, Req, Resp, St, S> IntoBidiStreamFactory<(ConnectRequest<Streaming<Req>>,), Req, Resp, S>
    for TonicCompatibleBidiStreamHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    S: Clone + Send + Sync + 'static,
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    fn into_bidi_stream_factory(
        self,
    ) -> Box<dyn Fn(&S) -> BoxedBidiStreamCall<Req, Resp> + Send + Sync> {
        let f = self.0;
        Box::new(move |_state: &S| {
            let f = f.clone();
            Box::new(
                move |_ctx: Option<RequestContext>, req: ConnectRequest<Streaming<Req>>| {
                    let fut = f(req);
                    Box::pin(async move {
                        fut.await.map(|response| {
                            let stream = response.into_inner().into_inner();
                            let boxed_stream: BoxedStream<Resp> = Box::pin(stream);
                            ConnectResponse::new(StreamBody::new(boxed_stream))
                        })
                    })
                },
            )
        })
    }
}

// =============== N-extractor IntoFactory implementations ===============

// Macro for 1-8 extractors
macro_rules! all_extractor_tuples {
    ($m:ident) => {
        $m!([A1]);
        $m!([A1, A2]);
        $m!([A1, A2, A3]);
        $m!([A1, A2, A3, A4]);
        $m!([A1, A2, A3, A4, A5]);
        $m!([A1, A2, A3, A4, A5, A6]);
        $m!([A1, A2, A3, A4, A5, A6, A7]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8]);
    };
}

macro_rules! impl_into_factory_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, Req, Resp, S, $($A,)+>
            IntoFactory<($($A,)+ ConnectRequest<Req>,), Req, Resp, S>
            for TonicCompatibleHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            Req: Send + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: Into<ConnectError>, )+
        {
            fn into_factory(self) -> Box<dyn Fn(&S) -> BoxedCall<Req, Resp> + Send + Sync> {
                let f = self.0;
                Box::new(move |state_ref: &S| {
                    let f = f.clone();
                    let state = state_ref.clone();
                    Box::new(move |ctx: Option<RequestContext>, req: ConnectRequest<Req>| {
                        let f = f.clone();
                        let state = state.clone();
                        Box::pin(async move {
                            // Extractors require RequestContext - error if middleware not applied
                            let ctx = ctx.ok_or_else(|| ConnectError::new(
                                crate::error::Code::Internal,
                                "middleware required for handlers with extractor",
                            ))?;
                            let mut parts = ctx.into_parts();
                            $(
                                #[allow(non_snake_case)]
                                let $A = $A::from_request_parts(&mut parts, &state)
                                    .await
                                    .map_err(Into::into)?;
                            )+
                            f($($A,)+ req).await
                        })
                    })
                })
            }
        }
    };
}

macro_rules! impl_into_stream_factory_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, Req, Resp, St, S, $($A,)+>
            IntoStreamFactory<($($A,)+ ConnectRequest<Req>,), Req, Resp, S>
            for TonicCompatibleStreamHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
            St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            Req: Send + Sync + 'static,
            Resp: Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: Into<ConnectError>, )+
        {
            fn into_stream_factory(self) -> Box<dyn Fn(&S) -> BoxedStreamCall<Req, Resp> + Send + Sync> {
                let f = self.0;
                Box::new(move |state_ref: &S| {
                    let f = f.clone();
                    let state = state_ref.clone();
                    Box::new(move |ctx: Option<RequestContext>, req: ConnectRequest<Req>| {
                        let f = f.clone();
                        let state = state.clone();
                        Box::pin(async move {
                            // Extractors require RequestContext - error if middleware not applied
                            let ctx = ctx.ok_or_else(|| ConnectError::new(
                                crate::error::Code::Internal,
                                "middleware required for handlers with extractor",
                            ))?;
                            let mut parts = ctx.into_parts();
                            $(
                                #[allow(non_snake_case)]
                                let $A = $A::from_request_parts(&mut parts, &state)
                                    .await
                                    .map_err(Into::into)?;
                            )+
                            f($($A,)+ req).await.map(|response| {
                                let stream = response.into_inner().into_inner();
                                let boxed_stream: BoxedStream<Resp> = Box::pin(stream);
                                ConnectResponse::new(StreamBody::new(boxed_stream))
                            })
                        })
                    })
                })
            }
        }
    };
}

macro_rules! impl_into_client_stream_factory_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, Req, Resp, S, $($A,)+>
            IntoClientStreamFactory<($($A,)+ ConnectRequest<Streaming<Req>>,), Req, Resp, S>
            for TonicCompatibleClientStreamHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            Req: Send + Sync + 'static,
            Resp: Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: Into<ConnectError>, )+
        {
            fn into_client_stream_factory(self) -> Box<dyn Fn(&S) -> BoxedClientStreamCall<Req, Resp> + Send + Sync> {
                let f = self.0;
                Box::new(move |state_ref: &S| {
                    let f = f.clone();
                    let state = state_ref.clone();
                    Box::new(move |ctx: Option<RequestContext>, req: ConnectRequest<Streaming<Req>>| {
                        let f = f.clone();
                        let state = state.clone();
                        Box::pin(async move {
                            let ctx = ctx.ok_or_else(|| ConnectError::new(
                                crate::error::Code::Internal,
                                "middleware required for handlers with extractor",
                            ))?;
                            let mut parts = ctx.into_parts();
                            $(
                                #[allow(non_snake_case)]
                                let $A = $A::from_request_parts(&mut parts, &state)
                                    .await
                                    .map_err(Into::into)?;
                            )+
                            f($($A,)+ req).await
                        })
                    })
                })
            }
        }
    };
}

macro_rules! impl_into_bidi_stream_factory_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, Req, Resp, St, S, $($A,)+>
            IntoBidiStreamFactory<($($A,)+ ConnectRequest<Streaming<Req>>,), Req, Resp, S>
            for TonicCompatibleBidiStreamHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
            St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            Req: Send + Sync + 'static,
            Resp: Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: Into<ConnectError>, )+
        {
            fn into_bidi_stream_factory(self) -> Box<dyn Fn(&S) -> BoxedBidiStreamCall<Req, Resp> + Send + Sync> {
                let f = self.0;
                Box::new(move |state_ref: &S| {
                    let f = f.clone();
                    let state = state_ref.clone();
                    Box::new(move |ctx: Option<RequestContext>, req: ConnectRequest<Streaming<Req>>| {
                        let f = f.clone();
                        let state = state.clone();
                        Box::pin(async move {
                            let ctx = ctx.ok_or_else(|| ConnectError::new(
                                crate::error::Code::Internal,
                                "middleware required for handlers with extractor",
                            ))?;
                            let mut parts = ctx.into_parts();
                            $(
                                #[allow(non_snake_case)]
                                let $A = $A::from_request_parts(&mut parts, &state)
                                    .await
                                    .map_err(Into::into)?;
                            )+
                            f($($A,)+ req).await.map(|response| {
                                let stream = response.into_inner().into_inner();
                                let boxed_stream: BoxedStream<Resp> = Box::pin(stream);
                                ConnectResponse::new(StreamBody::new(boxed_stream))
                            })
                        })
                    })
                })
            }
        }
    };
}

// Generate implementations for 1-8 extractors
all_extractor_tuples!(impl_into_factory_with_extractors);
all_extractor_tuples!(impl_into_stream_factory_with_extractors);
all_extractor_tuples!(impl_into_client_stream_factory_with_extractors);
all_extractor_tuples!(impl_into_bidi_stream_factory_with_extractors);

// =============== Handler implementations for Connect protocol ===============

// Implement Handler for TonicCompatibleHandlerWrapper - no extractors variant
impl<F, Fut, Req, Resp> Handler<(ConnectRequest<Req>,), ()> for TonicCompatibleHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    ConnectRequest<Req>: FromRequest<()>,
    Resp: prost::Message + serde::Serialize + Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, state: ()) -> Self::Future {
        Box::pin(async move {
            let ctx = req
                .extensions()
                .get::<Context>()
                .cloned()
                .unwrap_or_default();

            let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            let result = (self.0)(connect_req).await;

            match result {
                Ok(response) => response.into_response_with_context(&ctx),
                Err(err) => err.into_response_with_protocol(ctx.protocol),
            }
        })
    }
}

// =============== N-extractor Handler implementations ===============

macro_rules! impl_handler_for_tonic_wrapper_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, S, Req, Resp, $($A,)+>
            Handler<($($A,)+ ConnectRequest<Req>,), S>
            for TonicCompatibleHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: IntoResponse + 'static, )+
            ConnectRequest<Req>: FromRequest<S>,
            Resp: prost::Message + serde::Serialize + Send + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    let ctx = req.extensions().get::<Context>().cloned().unwrap_or_default();
                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts
                    $(
                        #[allow(non_snake_case)]
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(v) => v,
                            Err(e) => return handle_extractor_rejection(e, ctx.protocol),
                        };
                    )+

                    // Reconstruct request for body
                    let req = Request::from_parts(parts, body);
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(v) => v,
                        Err(e) => return e.into_response(),
                    };

                    match (self.0)($($A,)+ connect_req).await {
                        Ok(resp) => resp.into_response_with_context(&ctx),
                        Err(e) => e.into_response_with_protocol(ctx.protocol),
                    }
                })
            }
        }
    };
}

// Generate implementations for 1-8 extractors
all_extractor_tuples!(impl_handler_for_tonic_wrapper_with_extractors);

/// Creates a POST method router for tonic-compatible unary RPC handlers.
pub fn post_tonic_unary<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TonicCompatibleHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(TonicCompatibleHandlerWrapper(f))
}

/// Creates a POST method router for tonic-compatible streaming RPC handlers.
pub fn post_tonic_stream<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TonicCompatibleStreamHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(TonicCompatibleStreamHandlerWrapper(f))
}

// =============== Streaming Handler Implementations ===============

// Implement Handler for TonicCompatibleStreamHandlerWrapper (no-state)
impl<F, Fut, Req, Resp, St> Handler<(ConnectRequest<Req>,), ()>
    for TonicCompatibleStreamHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    ConnectRequest<Req>: FromRequest<()>,
    Req: Send + Sync + 'static,
    Resp: prost::Message + serde::Serialize + Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            let ctx = req
                .extensions()
                .get::<Context>()
                .cloned()
                .unwrap_or_default();

            let connect_req = match ConnectRequest::<Req>::from_request(req, &()).await {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            let result = (self.0)(connect_req).await;

            match result {
                Ok(response) => response.into_response_with_context(&ctx),
                Err(err) => {
                    let use_proto = ctx.protocol.is_proto();
                    err.into_streaming_response(use_proto)
                }
            }
        })
    }
}

// N-extractor streaming handler implementations
macro_rules! impl_handler_for_tonic_stream_wrapper_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, S, Req, Resp, St, $($A,)+>
            Handler<($($A,)+ ConnectRequest<Req>,), S>
            for TonicCompatibleStreamHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
            St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: IntoResponse + 'static, )+
            ConnectRequest<Req>: FromRequest<S>,
            Req: Send + Sync + 'static,
            Resp: prost::Message + serde::Serialize + Send + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    let ctx = req.extensions().get::<Context>().cloned().unwrap_or_default();
                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts
                    $(
                        #[allow(non_snake_case)]
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(v) => v,
                            Err(e) => return handle_extractor_rejection(e, ctx.protocol),
                        };
                    )+

                    // Reconstruct request for body
                    let req = Request::from_parts(parts, body);
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(v) => v,
                        Err(e) => return e.into_response(),
                    };

                    match (self.0)($($A,)+ connect_req).await {
                        Ok(resp) => resp.into_response_with_context(&ctx),
                        Err(e) => {
                            let use_proto = ctx.protocol.is_proto();
                            e.into_streaming_response(use_proto)
                        }
                    }
                })
            }
        }
    };
}

// Generate implementations for 1-8 extractors
all_extractor_tuples!(impl_handler_for_tonic_stream_wrapper_with_extractors);

// =============== Client Streaming Handler Implementations ===============

/// Validate protocol for streaming handlers. Returns error response if invalid.
fn validate_streaming_protocol(ctx: &Context) -> Option<Response> {
    validate_streaming_content_type(ctx.protocol).map(|err| {
        let use_proto = ctx.protocol.is_proto();
        err.into_streaming_response(use_proto)
    })
}

// Implement Handler for TonicCompatibleClientStreamHandlerWrapper (no-state)
impl<F, Fut, Req, Resp> Handler<(ConnectRequest<Streaming<Req>>,), ()>
    for TonicCompatibleClientStreamHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    Req: Message + DeserializeOwned + Default + Send + 'static,
    Resp: Message + serde::Serialize + Send + Clone + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            let ctx = req
                .extensions()
                .get::<Context>()
                .cloned()
                .unwrap_or_default();

            // Validate: streaming handlers only accept streaming content-types
            if let Some(err_response) = validate_streaming_protocol(&ctx) {
                return err_response;
            }

            let streaming_req = match ConnectRequest::<Streaming<Req>>::from_request(req, &()).await
            {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            let result = (self.0)(streaming_req).await;

            match result {
                Ok(response) => response.into_streaming_response_with_context(&ctx),
                Err(err) => {
                    let use_proto = ctx.protocol.is_proto();
                    err.into_streaming_response(use_proto)
                }
            }
        })
    }
}

// N-extractor client streaming handler implementations
macro_rules! impl_handler_for_tonic_client_stream_wrapper_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, S, Req, Resp, $($A,)+>
            Handler<($($A,)+ ConnectRequest<Streaming<Req>>,), S>
            for TonicCompatibleClientStreamHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: IntoResponse + 'static, )+
            Req: Message + DeserializeOwned + Default + Send + 'static,
            Resp: Message + serde::Serialize + Send + Clone + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    let ctx = req.extensions().get::<Context>().cloned().unwrap_or_default();

                    // Validate: streaming handlers only accept streaming content-types
                    if let Some(err_response) = validate_streaming_protocol(&ctx) {
                        return err_response;
                    }

                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts
                    $(
                        #[allow(non_snake_case)]
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(v) => v,
                            Err(e) => return handle_extractor_rejection(e, ctx.protocol),
                        };
                    )+

                    // Reconstruct request for body
                    let req = Request::from_parts(parts, body);
                    let streaming_req = match ConnectRequest::<Streaming<Req>>::from_request(req, &state).await {
                        Ok(v) => v,
                        Err(e) => return e.into_response(),
                    };

                    match (self.0)($($A,)+ streaming_req).await {
                        Ok(resp) => resp.into_streaming_response_with_context(&ctx),
                        Err(e) => {
                            let use_proto = ctx.protocol.is_proto();
                            e.into_streaming_response(use_proto)
                        }
                    }
                })
            }
        }
    };
}

// Generate implementations for 1-8 extractors
all_extractor_tuples!(impl_handler_for_tonic_client_stream_wrapper_with_extractors);

/// Creates a POST method router for tonic-compatible client streaming RPC handlers.
pub fn post_tonic_client_stream<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TonicCompatibleClientStreamHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(TonicCompatibleClientStreamHandlerWrapper(f))
}

// =============== Bidi Streaming Handler Implementations ===============

// Implement Handler for TonicCompatibleBidiStreamHandlerWrapper (no-state)
impl<F, Fut, Req, Resp, St> Handler<(ConnectRequest<Streaming<Req>>,), ()>
    for TonicCompatibleBidiStreamHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    Req: Message + DeserializeOwned + Default + Send + 'static,
    Resp: Message + serde::Serialize + Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            let ctx = req
                .extensions()
                .get::<Context>()
                .cloned()
                .unwrap_or_default();

            // Validate: streaming handlers only accept streaming content-types
            if let Some(err_response) = validate_streaming_protocol(&ctx) {
                return err_response;
            }

            let streaming_req = match ConnectRequest::<Streaming<Req>>::from_request(req, &()).await
            {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            let result = (self.0)(streaming_req).await;

            match result {
                Ok(response) => response.into_response_with_context(&ctx),
                Err(err) => {
                    let use_proto = ctx.protocol.is_proto();
                    err.into_streaming_response(use_proto)
                }
            }
        })
    }
}

// N-extractor bidi streaming handler implementations
macro_rules! impl_handler_for_tonic_bidi_stream_wrapper_with_extractors {
    ([$($A:ident),+]) => {
        impl<F, Fut, S, Req, Resp, St, $($A,)+>
            Handler<($($A,)+ ConnectRequest<Streaming<Req>>,), S>
            for TonicCompatibleBidiStreamHandlerWrapper<F>
        where
            F: Fn($($A,)+ ConnectRequest<Streaming<Req>>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
            St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            $( $A: FromRequestParts<S> + Send + 'static,
               <$A as FromRequestParts<S>>::Rejection: IntoResponse + 'static, )+
            Req: Message + DeserializeOwned + Default + Send + 'static,
            Resp: Message + serde::Serialize + Send + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    let ctx = req.extensions().get::<Context>().cloned().unwrap_or_default();

                    // Validate: streaming handlers only accept streaming content-types
                    if let Some(err_response) = validate_streaming_protocol(&ctx) {
                        return err_response;
                    }

                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts
                    $(
                        #[allow(non_snake_case)]
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(v) => v,
                            Err(e) => return handle_extractor_rejection(e, ctx.protocol),
                        };
                    )+

                    // Reconstruct request for body
                    let req = Request::from_parts(parts, body);
                    let streaming_req = match ConnectRequest::<Streaming<Req>>::from_request(req, &state).await {
                        Ok(v) => v,
                        Err(e) => return e.into_response(),
                    };

                    match (self.0)($($A,)+ streaming_req).await {
                        Ok(resp) => resp.into_response_with_context(&ctx),
                        Err(e) => {
                            let use_proto = ctx.protocol.is_proto();
                            e.into_streaming_response(use_proto)
                        }
                    }
                })
            }
        }
    };
}

// Generate implementations for 1-8 extractors
all_extractor_tuples!(impl_handler_for_tonic_bidi_stream_wrapper_with_extractors);

/// Creates a POST method router for tonic-compatible bidirectional streaming RPC handlers.
/// Requires HTTP/2 for full-duplex communication.
pub fn post_tonic_bidi_stream<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TonicCompatibleBidiStreamHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(TonicCompatibleBidiStreamHandlerWrapper(f))
}

/// Creates an unimplemented handler that returns ConnectError::unimplemented for the given method name
pub fn unimplemented_boxed_call<Req, Resp>() -> BoxedCall<Req, Resp>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    Box::new(|_ctx: Option<RequestContext>, _req: ConnectRequest<Req>| {
        Box::pin(async move { Err(ConnectError::new_unimplemented()) })
    })
}

/// Creates an unimplemented handler that returns ConnectError::unimplemented for streaming methods
pub fn unimplemented_boxed_stream_call<Req, Resp>() -> BoxedStreamCall<Req, Resp>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    Box::new(|_ctx: Option<RequestContext>, _req: ConnectRequest<Req>| {
        Box::pin(async move { Err(ConnectError::new_unimplemented()) })
    })
}

/// Creates an unimplemented handler that returns ConnectError::unimplemented for client streaming methods
pub fn unimplemented_boxed_client_stream_call<Req, Resp>() -> BoxedClientStreamCall<Req, Resp>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    Box::new(
        |_ctx: Option<RequestContext>, _req: ConnectRequest<Streaming<Req>>| {
            Box::pin(async move { Err(ConnectError::new_unimplemented()) })
        },
    )
}

/// Creates an unimplemented handler that returns ConnectError::unimplemented for bidirectional streaming methods
pub fn unimplemented_boxed_bidi_stream_call<Req, Resp>() -> BoxedBidiStreamCall<Req, Resp>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    Box::new(
        |_ctx: Option<RequestContext>, _req: ConnectRequest<Streaming<Req>>| {
            Box::pin(async move { Err(ConnectError::new_unimplemented()) })
        },
    )
}
