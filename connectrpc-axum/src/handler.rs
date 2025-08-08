use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    handler::Handler,
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use std::{future::Future, pin::Pin};

use crate::{error::ConnectError, request::ConnectRequest, response::ConnectResponse};

#[cfg(feature = "tonic")]
mod tonic;
#[cfg(feature = "tonic")]
pub use tonic::*;

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
    ConnectResponse<Resp>: IntoResponse + Send + Sync + 'static,
    Req: Send + Sync + 'static,
    Resp: Send + Clone + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move {
            // Extract the ConnectRequest (body only)
            let connect_req = match ConnectRequest::<Req>::from_request(req, &()).await {
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

            // Constraints on extractors
            $( $A: FromRequestParts<S> + Send + Sync + 'static, )*
            ConnectRequest<Req>: FromRequest<S>,
            Req: Send + Sync + 'static,
            S: Send + Sync + 'static,

            // Response constraints
            ConnectResponse<Resp>: IntoResponse + Send + Sync + 'static,
            Resp: Send + Clone + Sync + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            #[allow(unused_mut)]
            fn call(self, req: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    // Split the request into parts and body
                    let (mut parts, body) = req.into_parts();

                    // Extract each FromRequestParts extractor
                    $(
                        let $A = match $A::from_request_parts(&mut parts, &state).await {
                            Ok(value) => value,
                            Err(rejection) => return rejection.into_response(),
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
                    let result = (self.0)($($A,)* connect_req).await;

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

#[allow(non_snake_case)]
mod generated_handler_impls {
    use super::*;
    // Use the non-empty macro since we handle the empty case separately
    all_tuples_nonempty!(impl_handler_for_connect_handler_wrapper);
}

// =============== TonicCompatibleHandlerWrapper implementations ===============

/// Creates a POST method router from a ConnectHandler function (flexible mode)
pub fn post_connect<F, T, S>(f: F) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ConnectHandlerWrapper<F>: Handler<T, S>,
    T: 'static,
{
    axum::routing::post(ConnectHandlerWrapper(f))
}
