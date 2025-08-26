//! ConnectHandler trait for type-safe Connect RPC handlers.
//!
//! This module provides a `ConnectHandler` trait similar to Axum's `Handler` trait
//! but ensures that handlers have `ConnectRequest<T>` as their last parameter.
//! It supports variadic parameters and provides a bridge to Axum's Handler trait.
//!
//! # Basic Usage
//!
//! ## Simple Handler (no extractors)
//!
//! ```rust,ignore
//! use connectrpc_axum::prelude::*;
//!
//! async fn say_hello(
//!     ConnectRequest(req): ConnectRequest<HelloRequest>
//! ) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
//!     let response = HelloResponse {
//!         message: format!("Hello, {}!", req.name),
//!     };
//!     Ok(ConnectResponse(response))
//! }
//!
//! // Wrap the handler with `post_connect` so Axum can accept it
//! let app = axum::Router::new()
//!     .route("/my.Service/SayHello", post_connect(say_hello));
//! ```
//!
//! ## Handler with State
//!
//! ```rust,ignore
//! use connectrpc_axum::prelude::*;
//! use axum::extract::State;
//!
//! #[derive(Clone)]
//! struct AppState {
//!     db: Database,
//! }
//!
//! async fn get_user(
//!     State(state): State<AppState>,
//!     ConnectRequest(req): ConnectRequest<GetUserRequest>
//! ) -> Result<ConnectResponse<GetUserResponse>, ConnectError> {
//!     let user = state.db.get_user(req.id).await?;
//!     Ok(ConnectResponse(GetUserResponse { user }))
//! }
//!
//! let app = axum::Router::new()
//!     .route("/my.Service/GetUser", post_connect(get_user))
//!     .with_state(AppState { db: Database::new() });
//! ```
//!
//! ## Handler with Multiple Extractors
//!
//! ```rust,ignore
//! use connectrpc_axum::prelude::*;
//! use axum::extract::{State, Query, Path};
//!
//! async fn complex_handler(
//!     State(db): State<Database>,
//!     Path(id): Path<String>,
//!     Query(params): Query<FilterParams>,
//!     ConnectRequest(req): ConnectRequest<MyRequest>  // Must be last!
//! ) -> Result<ConnectResponse<MyResponse>, ConnectError> {
//!     // Handler implementation
//!     todo!()
//! }
//! ```
//!
//! # Important Notes
//!
//! - `ConnectRequest<T>` must always be the LAST parameter since it consumes the request body
//! - All other extractors must implement `FromRequestParts` and come before `ConnectRequest`
//! - Handlers automatically implement `ConnectHandler` and can be wrapped with `post_connect`
//!   (or [`ConnectHandlerWrapper`]) to work with Axum's routing APIs
//! - The bridge implementation ensures full compatibility with Axum's ecosystem

use crate::extractor::ConnectRequest;
use axum::{
    extract::{FromRequest, Request},
    handler::Handler,
    response::{IntoResponse, Response},
};
use std::future::Future;

// Replicate all_the_tuples! since Axum's is internal
#[rustfmt::skip]
macro_rules! all_the_tuples {
    ($name:ident) => {
        $name!([]);
        $name!([T1]);
        $name!([T1, T2]);
        $name!([T1, T2, T3]);
        $name!([T1, T2, T3, T4]);
        $name!([T1, T2, T3, T4, T5]);
        $name!([T1, T2, T3, T4, T5, T6]);
        $name!([T1, T2, T3, T4, T5, T6, T7]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16]);
    };
}

/// Handler trait for functions whose last parameter is `ConnectRequest<T>`.
///
/// This trait is automatically implemented for functions that have `ConnectRequest<T>`
/// as their last parameter. These functions will also automatically implement
/// Axum's `Handler` trait through our bridge implementation.
///
/// You don't need to implement this trait manually - it's automatically
/// implemented for qualifying functions via the macro system.
pub trait ConnectHandler<T, S>: Clone + Send + 'static {
    type Future: Future<Output = Response> + Send;

    fn call(self, req: Request, state: S) -> Self::Future;
}

// Note: We intentionally do not expose a marker trait like
// `ConnectHandlerAny<S>` here. The blanket `Handler` impl for
// `ConnectHandlerWrapper<H>` below allows the router to accept
// any handler whose last argument is `ConnectRequest<_>`, without
// naming the extractor tuple in bounds.

// Implement ConnectHandler for all function arities where the last parameter is ConnectRequest<Req>
macro_rules! impl_connect_handler {
    ([$($ty:ident),*]) => {
        impl<F, Fut, Res, S, Req, $($ty,)*> ConnectHandler<($($ty,)* ConnectRequest<Req>,), S> for F
        where
            F: Fn($($ty,)* ConnectRequest<Req>) -> Fut + Clone + Send + 'static,
            Fut: Future<Output = Res> + Send,
            Res: IntoResponse,
            S: Clone + Send + Sync + 'static,
            ConnectRequest<Req>: axum::extract::FromRequest<S>,
            Req: Send + 'static,
            $($ty: axum::extract::FromRequestParts<S> + Send + 'static,)*
        {
            type Future = std::pin::Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    // Split the request into parts and body for extraction
                    let (mut parts, body) = req.into_parts();

                    // Extract all the FromRequestParts extractors first
                    $(
                        let $ty = match $ty::from_request_parts(&mut parts, &state).await {
                            Ok(extractor) => extractor,
                            Err(rejection) => {
                                return rejection.into_response();
                            }
                        };
                    )*

                    // Reconstruct the request for ConnectRequest
                    let req = Request::from_parts(parts, body);

                    // Extract the ConnectRequest (this consumes the body)
                    let connect_req = match ConnectRequest::<Req>::from_request(req, &state).await {
                        Ok(req) => req,
                        Err(rejection) => {
                            return rejection.into_response();
                        }
                    };

                    // Call the handler function and convert result to Response
                    let result = self($($ty,)* connect_req).await;
                    result.into_response()
                })
            }
        }
    };
}

// Generate implementations for all tuple sizes
all_the_tuples!(impl_connect_handler);

// Bridge wrapper to work around orphan rule
#[derive(Clone)]
pub struct ConnectHandlerWrapper<H>(pub H);

// We need specific implementations for each tuple arity rather than a generic one
// This avoids the inference issues with the generic T parameter
macro_rules! impl_handler_for_wrapper {
    ([$($ty:ident),*]) => {
        impl<H, S, Req, $($ty,)*> Handler<($($ty,)* ConnectRequest<Req>,), S> for ConnectHandlerWrapper<H>
        where
            H: ConnectHandler<($($ty,)* ConnectRequest<Req>,), S> + Sync,
            ($($ty,)* ConnectRequest<Req>,): Send + 'static,
            S: Send + Sync + 'static,
        {
            type Future = H::Future;

            fn call(self, req: Request, state: S) -> Self::Future {
                ConnectHandler::call(self.0, req, state)
            }
        }
    };
}

all_the_tuples!(impl_handler_for_wrapper);

use tower::Service;

/// Service wrapper for Connect handlers.
/// This converts a handler into a Tower Service that can be used with route_service.
#[derive(Clone)]
pub struct ConnectService<H, T, S> {
    handler: H,
    state: S,
    _phantom: std::marker::PhantomData<T>,
}

impl<H, T, S> ConnectService<H, T, S> {
    pub fn new(handler: H, state: S) -> Self {
        Self {
            handler,
            state,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<H, T, S> Service<Request> for ConnectService<H, T, S>
where
    H: Handler<T, S> + Clone + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Response = Response;
    type Error = std::convert::Infallible;
    type Future =
        std::pin::Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let handler = self.handler.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let response = handler.call(req, state).await;
            Ok(response)
        })
    }
}

/// Convenience function to create a ConnectService.
pub fn connect_service<H, T, S>(handler: H, state: S) -> ConnectService<H, T, S>
where
    H: Handler<T, S> + Clone + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    ConnectService::new(handler, state)
}

/// Helper function to wrap handlers for use with axum routing.
/// This provides the necessary type inference to resolve the Handler trait implementation.
pub fn connect_handler<H>(handler: H) -> ConnectHandlerWrapper<H> {
    ConnectHandlerWrapper(handler)
}

/// Helper function to wrap a Connect handler into a POST method router.
/// This avoids requiring users to manually wrap handlers with [`ConnectHandlerWrapper`].
pub fn post_connect<H, T, S>(handler: H) -> axum::routing::MethodRouter<S>
where
    H: ConnectHandler<T, S> + Sync,
    ConnectHandlerWrapper<H>: Handler<T, S>,
    T: Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    axum::routing::post(ConnectHandlerWrapper(handler))
}
