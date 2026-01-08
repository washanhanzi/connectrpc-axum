use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    handler::Handler,
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use std::{any::Any, future::Future, pin::Pin};

use crate::{
    context::{
        ConnectContext, RequestProtocol, validate_streaming_content_type,
        validate_unary_content_type,
    },
    error::ConnectError,
    message::{ConnectRequest, ConnectResponse, ConnectStreamingRequest, StreamBody, Streaming},
};
use futures::Stream;
use prost::Message;
use serde::de::DeserializeOwned;

/// Handle extractor rejections with protocol-aware encoding.
///
/// If the rejection is a `ConnectError`, it's encoded using the protocol from the request.
/// Otherwise, the rejection is returned as-is via `IntoResponse`. This allows extractors
/// to return non-Connect responses like HTTP redirects for authentication flows.
pub(crate) fn handle_extractor_rejection<R>(rejection: R, protocol: RequestProtocol) -> Response
where
    R: IntoResponse + Any,
{
    let rejection_any: Box<dyn Any> = Box::new(rejection);

    match rejection_any.downcast::<ConnectError>() {
        Ok(connect_err) => connect_err.into_response_with_protocol(protocol),
        Err(any_box) => {
            tracing::warn!(
                "Extractor rejection is not ConnectError, returning as-is. \
                 If this is unintentional, consider using an extractor that returns ConnectError."
            );
            // Downcast back to original type to call into_response
            any_box
                .downcast::<R>()
                .map(|r| r.into_response())
                .unwrap_or_else(|_| {
                    // Shouldn't happen, but fallback to internal error
                    ConnectError::new_internal("extractor rejection")
                        .into_response_with_protocol(protocol)
                })
        }
    }
}

// Note: Tonic handler types are now in crate::tonic module

/// Validate protocol for unary handlers. Returns error response if invalid.
///
/// Unary handlers only accept unary content-types (`application/json`, `application/proto`).
/// Streaming content-types are rejected with `Code::Unknown`.
fn validate_unary_protocol(ctx: &ConnectContext) -> Option<Response> {
    validate_unary_content_type(ctx.protocol)
        .map(|err| err.into_response_with_protocol(ctx.protocol))
}

/// Validate protocol for streaming handlers. Returns error response if invalid.
///
/// Streaming handlers only accept streaming content-types
/// (`application/connect+json`, `application/connect+proto`).
/// Unary content-types are rejected with `Code::Unknown`.
fn validate_streaming_protocol(ctx: &ConnectContext) -> Option<Response> {
    validate_streaming_content_type(ctx.protocol).map(|err| {
        let use_proto = ctx.protocol.is_proto();
        err.into_streaming_response(use_proto)
    })
}

/// A wrapper that adapts ConnectHandler functions to work with Axum's Handler trait
#[derive(Clone)]
pub struct ConnectHandlerWrapper<F>(pub F);

/// Type alias for compatibility with generated code
pub type ConnectHandler<F> = ConnectHandlerWrapper<F>;

// Macro for non-empty tuples only (excludes empty case)
macro_rules! all_tuples_nonempty {
    ($m:ident) => {
        $m!([A1]);
        $m!([A1, A2]);
        $m!([A1, A2, A3]);
        $m!([A1, A2, A3, A4]);
        $m!([A1, A2, A3, A4, A5]);
        $m!([A1, A2, A3, A4, A5, A6]);
        $m!([A1, A2, A3, A4, A5, A6, A7]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8, A9]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8, A9, A10]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13]);
        $m!([A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14]);
        $m!([
            A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15
        ]);
        $m!([
            A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16
        ]);
    };
}

// =============== 2) Handler implementations ===============

// Special case implementation for zero extractors (S must be ())
impl<F, Fut, Req, Resp> Handler<(ConnectRequest<Req>,), ()> for ConnectHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    ConnectRequest<Req>: FromRequest<()>,
    Req: Send + Sync + 'static,
    Resp: prost::Message + serde::Serialize + Send + Clone + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            // Extract pipeline context from extensions (set by ConnectLayer)
            let ctx = req
                .extensions()
                .get::<ConnectContext>()
                .cloned()
                .unwrap_or_default();

            // Validate: unary handlers only accept unary content-types
            if let Some(err_response) = validate_unary_protocol(&ctx) {
                return err_response;
            }

            // Extract the ConnectRequest (body only)
            let connect_req = match ConnectRequest::<Req>::from_request(req, &()).await {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            // Call the handler function
            // Note: Timeout is enforced by ConnectLayer, not here
            let result = (self.0)(connect_req).await;

            // Convert result to response using pipeline context
            match result {
                Ok(response) => response.into_response_with_context(&ctx),
                Err(err) => err.into_response_with_protocol(ctx.protocol),
            }
        })
    }
}

// Implement Handler for ConnectHandlerWrapper (flexible - allows any extractors)
// This now only handles non-empty tuples
macro_rules! impl_handler_for_connect_handler_wrapper {
    ([$($A:ident),*]) => {
        // Implement Handler for ConnectHandlerWrapper
        impl<F, Fut, S, Req, Resp, $($A,)*> Handler<($($A,)* ConnectRequest<Req>,), S>
            for ConnectHandlerWrapper<F>
        where
            F: Fn($($A,)* ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            S:Clone+Send+Sync+'static,

            // Constraints on extractors (rejection must be 'static for Any)
            $( $A: FromRequestParts<S> + Send + Sync + 'static,
               <$A as FromRequestParts<S>>::Rejection: 'static, )*
            ConnectRequest<Req>: FromRequest<S>,
            Req: Send + Sync + 'static,
            S: Send + Sync + 'static,

            // Response constraints
            Resp: prost::Message + serde::Serialize + Send + Clone + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            #[allow(unused_mut)]
            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    // Extract pipeline context from extensions (set by ConnectLayer)
                    let ctx = req
                        .extensions()
                        .get::<ConnectContext>()
                        .cloned()
                        .unwrap_or_default();

                    // Validate: unary handlers only accept unary content-types
                    if let Some(err_response) = validate_unary_protocol(&ctx) {
                        return err_response;
                    }

                    // Split the request into parts and body
                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts extractor
                    $(
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(value) => value,
                            Err(rejection) => return handle_extractor_rejection(rejection, ctx.protocol),
                        };
                    )*

                    // Reconstruct request for body extraction
                    let req = Request::from_parts(parts, body);

                    // Extract the ConnectRequest (body)
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(value) => value,
                        Err(err) => return err.into_response(),
                    };

                    // Call the handler function
                    // Note: Timeout is enforced by ConnectLayer, not here
                    let result = (self.0)($($A,)* connect_req).await;

                    // Convert result to response using pipeline context
                    match result {
                        Ok(response) => response.into_response_with_context(&ctx),
                        Err(err) => err.into_response_with_protocol(ctx.protocol),
                    }
                })
            }
        }

    };
}

#[allow(non_snake_case)]
mod generated_handler_impls {
    use super::*;
    // Use the non-empty macro since we handle the empty case separately
    all_tuples_nonempty!(impl_handler_for_connect_handler_wrapper);
}

// =============== Unified Handler: Server Streaming ===============
// Handler for: ConnectRequest<Req> -> ConnectResponse<StreamBody<St>>

/// Handler implementation for server streaming using the unified ConnectHandlerWrapper.
/// Input: single message, Output: stream of messages
impl<F, Fut, Req, Resp, St> Handler<(ConnectRequest<Req>, StreamBody<St>), ()>
    for ConnectHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    // Req must be a Message (not Streaming<T>) to distinguish from bidi streaming
    Req: Message + DeserializeOwned + Default + Send + Sync + 'static,
    Resp: Message + serde::Serialize + Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            let ctx = req
                .extensions()
                .get::<ConnectContext>()
                .cloned()
                .unwrap_or_default();

            // Validate: streaming handlers only accept streaming content-types
            if let Some(err_response) = validate_streaming_protocol(&ctx) {
                return err_response;
            }

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

// =============== Unified Handler: Client Streaming ===============
// Handler for: ConnectRequest<Streaming<Req>> -> ConnectResponse<Resp>

/// Handler implementation for client streaming using the unified ConnectHandlerWrapper.
/// Input: stream of messages, Output: single message
impl<F, Fut, Req, Resp> Handler<(ConnectRequest<Streaming<Req>>, Resp), ()>
    for ConnectHandlerWrapper<F>
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
                .get::<ConnectContext>()
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

            // Client streaming uses streaming framing for the response
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

// =============== Unified Handler: Bidi Streaming ===============
// Handler for: ConnectRequest<Streaming<Req>> -> ConnectResponse<StreamBody<St>>

/// Handler implementation for bidirectional streaming using the unified ConnectHandlerWrapper.
/// Input: stream of messages, Output: stream of messages
/// Note: Requires HTTP/2 for full-duplex communication.
impl<F, Fut, Req, Resp, St> Handler<(ConnectRequest<Streaming<Req>>, StreamBody<St>), ()>
    for ConnectHandlerWrapper<F>
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
                .get::<ConnectContext>()
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

/// Creates a POST method router for any Connect RPC handler.
///
/// This unified function automatically detects the RPC type based on the handler signature:
/// - `ConnectRequest<T>` → `ConnectResponse<U>` = Unary
/// - `ConnectRequest<T>` → `ConnectResponse<StreamBody<S>>` = Server streaming
/// - `ConnectRequest<Streaming<T>>` → `ConnectResponse<U>` = Client streaming
/// - `ConnectRequest<Streaming<T>>` → `ConnectResponse<StreamBody<S>>` = Bidi streaming
///
/// # Example
///
/// ```ignore
/// // Unary handler
/// async fn unary(req: ConnectRequest<MyReq>) -> Result<ConnectResponse<MyResp>, ConnectError> { ... }
///
/// // Server streaming handler
/// async fn server_stream(req: ConnectRequest<MyReq>) -> Result<ConnectResponse<StreamBody<impl Stream<...>>>, ConnectError> { ... }
///
/// // Client streaming handler
/// async fn client_stream(req: ConnectRequest<Streaming<MyReq>>) -> Result<ConnectResponse<MyResp>, ConnectError> { ... }
///
/// // Bidi streaming handler
/// async fn bidi_stream(req: ConnectRequest<Streaming<MyReq>>) -> Result<ConnectResponse<StreamBody<impl Stream<...>>>, ConnectError> { ... }
///
/// // All use the same post_connect function:
/// Router::new()
///     .route("/unary", post_connect(unary))
///     .route("/server", post_connect(server_stream))
///     .route("/client", post_connect(client_stream))
///     .route("/bidi", post_connect(bidi_stream))
/// ```
pub fn post_connect<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ConnectHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(ConnectHandlerWrapper(f))
}

/// Creates a POST method router for unary RPC handlers.
///
/// This is an alias for `post_connect` for backwards compatibility.
pub fn post_unary<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ConnectHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    post_connect(f)
}

/// Creates a GET method router for unary RPC handlers.
///
/// This enables idempotent unary RPCs to be accessed via GET requests,
/// which allows caching and is useful for browser use cases.
///
/// GET requests encode the message in query parameters:
/// - `connect=v1` (required when protocol header enforcement is enabled)
/// - `encoding=json|proto` (required)
/// - `message=<payload>` (required, URL-encoded)
/// - `base64=1` (optional, for binary payloads)
/// - `compression=gzip|identity` (optional)
///
/// To support both GET and POST, combine with `post_unary`:
/// ```ignore
/// .route("/path", get_unary(handler).merge(post_unary(handler)))
/// ```
pub fn get_unary<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ConnectHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::get(ConnectHandlerWrapper(f))
}

// =============== Streaming Handler Support ===============

/// A wrapper that adapts streaming ConnectHandler functions to work with Axum's Handler trait
#[derive(Clone)]
pub struct ConnectStreamHandlerWrapper<F>(pub F);

/// Type alias for compatibility with generated code
pub type ConnectStreamHandler<F> = ConnectStreamHandlerWrapper<F>;

// Special case implementation for zero extractors (S must be ())
impl<F, Fut, Req, Resp, St> Handler<(ConnectRequest<Req>,), ()> for ConnectStreamHandlerWrapper<F>
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
            // Extract pipeline context from extensions (set by ConnectLayer)
            let ctx = req
                .extensions()
                .get::<ConnectContext>()
                .cloned()
                .unwrap_or_default();

            // Validate: streaming handlers only accept streaming content-types
            if let Some(err_response) = validate_streaming_protocol(&ctx) {
                return err_response;
            }

            // Extract the ConnectRequest (body only)
            let connect_req = match ConnectRequest::<Req>::from_request(req, &()).await {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            // Call the handler function
            // Note: Timeout is enforced by ConnectLayer, not here
            let result = (self.0)(connect_req).await;

            // Convert result to response with context (enables per-message compression)
            // For streaming handlers, errors must use streaming framing (EndStream frame)
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

// Implement Handler for ConnectStreamHandlerWrapper (flexible - allows any extractors)
macro_rules! impl_handler_for_connect_stream_handler_wrapper {
    ([$($A:ident),*]) => {
        // Implement Handler for ConnectStreamHandlerWrapper
        impl<F, Fut, S, Req, Resp, St, $($A,)*> Handler<($($A,)* ConnectRequest<Req>,), S>
            for ConnectStreamHandlerWrapper<F>
        where
            F: Fn($($A,)* ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
            St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
            S: Clone + Send + Sync + 'static,

            // Constraints on extractors (rejection must be 'static for Any)
            $( $A: FromRequestParts<S> + Send + Sync + 'static,
               <$A as FromRequestParts<S>>::Rejection: 'static, )*
            ConnectRequest<Req>: FromRequest<S>,
            Req: Send + Sync + 'static,
            S: Send + Sync + 'static,

            // Response constraints
            Resp: prost::Message + serde::Serialize + Send + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            #[allow(unused_mut)]
            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    // Extract pipeline context from extensions (set by ConnectLayer)
                    let ctx = req
                        .extensions()
                        .get::<ConnectContext>()
                        .cloned()
                        .unwrap_or_default();

                    // Validate: streaming handlers only accept streaming content-types
                    if let Some(err_response) = validate_streaming_protocol(&ctx) {
                        return err_response;
                    }

                    // Split the request into parts and body
                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts extractor
                    $(
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(value) => value,
                            Err(rejection) => return handle_extractor_rejection(rejection, ctx.protocol),
                        };
                    )*

                    // Reconstruct request for body extraction
                    let req = Request::from_parts(parts, body);

                    // Extract the ConnectRequest (body)
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(value) => value,
                        Err(err) => return err.into_response(),
                    };

                    // Call the handler function
                    // Note: Timeout is enforced by ConnectLayer, not here
                    let result = (self.0)($($A,)* connect_req).await;

                    // Convert result to response with context (enables per-message compression)
                    // For streaming handlers, errors must use streaming framing (EndStream frame)
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
    };
}

#[allow(non_snake_case)]
mod generated_stream_handler_impls {
    use super::*;
    // Use the non-empty macro since we handle the empty case separately
    all_tuples_nonempty!(impl_handler_for_connect_stream_handler_wrapper);
}

/// Creates a POST method router for server streaming RPC handlers.
pub fn post_server_stream<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ConnectStreamHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(ConnectStreamHandlerWrapper(f))
}

// =============== Client Streaming Handler Support ===============

/// A wrapper that adapts client streaming handlers to work with Axum's Handler trait.
///
/// Client streaming: client sends a stream of messages, server responds with one message.
/// This is typically used by generated code for client streaming RPC methods.
#[derive(Clone)]
pub struct ConnectClientStreamHandlerWrapper<F>(pub F);

/// Type alias for compatibility with generated code
pub type ConnectClientStreamHandler<F> = ConnectClientStreamHandlerWrapper<F>;

impl<F, Fut, Req, Resp> Handler<(ConnectStreamingRequest<Req>,), ()>
    for ConnectClientStreamHandlerWrapper<F>
where
    F: Fn(ConnectStreamingRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    Req: Message + DeserializeOwned + Default + Send + 'static,
    Resp: Message + serde::Serialize + Send + Clone + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            // Extract pipeline context from extensions (set by ConnectLayer)
            let ctx = req
                .extensions()
                .get::<ConnectContext>()
                .cloned()
                .unwrap_or_default();

            // Validate: streaming handlers only accept streaming content-types
            if let Some(err_response) = validate_streaming_protocol(&ctx) {
                return err_response;
            }

            // Extract the streaming request
            let streaming_req = match ConnectStreamingRequest::<Req>::from_request(req, &()).await {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            // Call the handler function
            // Note: Timeout is enforced by ConnectLayer, not here
            let result = (self.0)(streaming_req).await;

            // Convert result to streaming response format with context
            // Client streaming uses streaming framing for the response
            // (single message frame + EndStreamResponse)
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

/// Creates a POST method router for client streaming RPC handlers.
///
/// Client streaming: client sends a stream of messages, server responds with one message.
pub fn post_client_stream<F, Req, Resp, Fut>(f: F) -> MethodRouter<()>
where
    F: Fn(ConnectStreamingRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    Req: Message + DeserializeOwned + Default + Send + 'static,
    Resp: Message + serde::Serialize + Send + Clone + Sync + 'static,
{
    axum::routing::post(ConnectClientStreamHandlerWrapper(f))
}

// =============== Bidirectional Streaming Handler Support ===============

/// A wrapper that adapts bidirectional streaming handlers to work with Axum's Handler trait.
///
/// Bidi streaming: both client and server send streams of messages.
/// This is typically used by generated code for bidirectional streaming RPC methods.
///
/// Note: Bidirectional streaming requires HTTP/2 for full-duplex communication.
#[derive(Clone)]
pub struct ConnectBidiStreamHandlerWrapper<F>(pub F);

/// Type alias for compatibility with generated code
pub type ConnectBidiStreamHandler<F> = ConnectBidiStreamHandlerWrapper<F>;

impl<F, Fut, Req, Resp, St> Handler<(ConnectStreamingRequest<Req>,), ()>
    for ConnectBidiStreamHandlerWrapper<F>
where
    F: Fn(ConnectStreamingRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    Req: Message + DeserializeOwned + Default + Send + 'static,
    Resp: Message + serde::Serialize + Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            // Extract pipeline context from extensions (set by ConnectLayer)
            let ctx = req
                .extensions()
                .get::<ConnectContext>()
                .cloned()
                .unwrap_or_default();

            // Validate: streaming handlers only accept streaming content-types
            if let Some(err_response) = validate_streaming_protocol(&ctx) {
                return err_response;
            }

            // Extract the streaming request
            let streaming_req = match ConnectStreamingRequest::<Req>::from_request(req, &()).await {
                Ok(value) => value,
                Err(err) => return err.into_response(),
            };

            // Call the handler function
            // Note: Timeout is enforced by ConnectLayer, not here
            let result = (self.0)(streaming_req).await;

            // Convert result to response with context (enables per-message compression)
            // For streaming responses, errors must use streaming framing (EndStream frame)
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

/// Creates a POST method router for bidirectional streaming RPC handlers.
///
/// Bidi streaming: both client and server send streams of messages.
/// Requires HTTP/2 for full-duplex communication.
pub fn post_bidi_stream<F, Req, Resp, St, Fut>(f: F) -> MethodRouter<()>
where
    F: Fn(ConnectStreamingRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<StreamBody<St>>, ConnectError>> + Send + 'static,
    St: Stream<Item = Result<Resp, ConnectError>> + Send + 'static,
    Req: Message + DeserializeOwned + Default + Send + 'static,
    Resp: Message + serde::Serialize + Send + Sync + 'static,
{
    axum::routing::post(ConnectBidiStreamHandlerWrapper(f))
}
