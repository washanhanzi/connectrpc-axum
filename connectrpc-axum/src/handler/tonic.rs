use axum::{
    extract::{FromRequest, Request},
    handler::Handler,
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use std::{future::Future, pin::Pin};

use crate::{error::ConnectError, request::ConnectRequest, response::ConnectResponse};

// =============== Factory trait for deferred handler boxing ===============
/// Strict wrapper that only allows Tonic-compatible handler patterns
///
/// Allowed handler signatures:
/// - `(ConnectRequest<Req>) -> impl Future<Result<ConnectResponse<Resp>, ConnectError>>`
/// - `(axum::extract::State<S>, ConnectRequest<Req>) -> impl Future<Result<ConnectResponse<Resp>, ConnectError>>`
#[derive(Clone)]
pub struct TonicCompatibleHandlerWrapper<F>(pub F);

// =============== Boxed callable and IntoFactory adapters ===============

/// Boxed callable used by generated tonic-compatible services.
pub type BoxedCall<Req, Resp> = Box<
    dyn Fn(
            ConnectRequest<Req>,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            >,
        > + Send
        + Sync,
>;

/// Adapter that turns a user handler `F` into a factory that, given `&S`, yields a `BoxedCall`.
/// Keyed by the extractor tuple `T` to select the appropriate implementation.
pub trait IntoFactory<T, Req, Resp, S> {
    fn into_factory(self) -> Box<dyn Fn(&S) -> BoxedCall<Req, Resp> + Send + Sync>;
}

/// Trait for tonic-compatible handlers with specific request/response types
// no-state: (ConnectRequest<Req>,)
// Implement IntoFactory for the wrapper, keyed by allowed extractor tuples.
impl<F, Fut, Req, Resp, S> IntoFactory<(ConnectRequest<Req>,), Req, Resp, S>
    for TonicCompatibleHandlerWrapper<F>
where
    F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_factory(self) -> Box<dyn Fn(&S) -> BoxedCall<Req, Resp> + Send + Sync> {
        let f = self.0.clone();
        Box::new(move |_state: &S| {
            let f = f.clone();
            Box::new(move |req: ConnectRequest<Req>| {
                let fut = f(req);
                Box::pin(async move { fut.await })
            })
        })
    }
}

// with-state: (State<S>, ConnectRequest<Req>)
impl<F, Fut, Req, Resp, S>
    IntoFactory<(axum::extract::State<S>, ConnectRequest<Req>), Req, Resp, S>
    for TonicCompatibleHandlerWrapper<F>
where
    F: Fn(axum::extract::State<S>, ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn into_factory(self) -> Box<dyn Fn(&S) -> BoxedCall<Req, Resp> + Send + Sync> {
        let f = self.0.clone();
        Box::new(move |state_ref: &S| {
            let f = f.clone();
            let state_cloned = state_ref.clone();
            Box::new(move |req: ConnectRequest<Req>| {
                let state = axum::extract::State(state_cloned.clone());
                let fut = f(state, req);
                Box::pin(async move { fut.await })
            })
        })
    }
}

// Implement Handler for TonicCompatibleHandlerWrapper (strict - only 2 variants allowed)
macro_rules! impl_handler_for_tonic_compatible_wrapper {
    // Variant 1: Just ConnectRequest
    () => {
        impl<F, Fut, Req, Resp> Handler<(ConnectRequest<Req>,), ()>
            for TonicCompatibleHandlerWrapper<F>
        where
            F: Fn(ConnectRequest<Req>) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            ConnectRequest<Req>: FromRequest<()>,
            ConnectResponse<Resp>: IntoResponse,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: ()) -> Self::Future {
                Box::pin(async move {
                    // Extract the ConnectRequest (body only)
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(value) => value,
                        Err(err) => return err.into_response(),
                    };

                    // Call the handler function
                    let result = (self.0)(connect_req).await;

                    // Convert result to response
                    match result {
                        Ok(response) => response.into_response(),
                        Err(err) => err.into_response(),
                    }
                })
            }
        }
    };

    // Variant 2: State + ConnectRequest
    ([State]) => {
        impl<F, Fut, S, Req, Resp> Handler<(axum::extract::State<S>, ConnectRequest<Req>), S>
            for TonicCompatibleHandlerWrapper<F>
        where
            F: Fn(axum::extract::State<S>, ConnectRequest<Req>) -> Fut
                + Clone
                + Send
                + Sync
                + 'static,
            Fut: Future<Output = Result<ConnectResponse<Resp>, ConnectError>> + Send + 'static,
            ConnectRequest<Req>: FromRequest<S>,
            S: Clone + Send + Sync + 'static,
            ConnectResponse<Resp>: IntoResponse,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    // Clone state for the extractor and extract body directly
                    let state_extractor = axum::extract::State(state.clone());

                    // Extract the ConnectRequest (body)
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(value) => value,
                        Err(err) => return err.into_response(),
                    };

                    // Call the handler function
                    let result = (self.0)(state_extractor, connect_req).await;

                    // Convert result to response
                    match result {
                        Ok(response) => response.into_response(),
                        Err(err) => err.into_response(),
                    }
                })
            }
        }
    };
}

// Generate the two allowed implementations
impl_handler_for_tonic_compatible_wrapper!();
impl_handler_for_tonic_compatible_wrapper!([State]);

/// Creates a POST method router from a TonicCompatible handler function (strict mode)
pub fn post_connect_tonic<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TonicCompatibleHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(TonicCompatibleHandlerWrapper(f))
}

/// Creates an unimplemented handler that returns ConnectError::unimplemented for the given method name
/// Convenience: produce an unimplemented boxed call for a method.
pub fn unimplemented_boxed_call<Req, Resp>() -> BoxedCall<Req, Resp>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
{
    Box::new(|_req: ConnectRequest<Req>| {
        Box::pin(async move { Err(ConnectError::new_unimplemented()) })
    })
}
