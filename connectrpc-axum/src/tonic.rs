//! Content-type switch utilities: dispatcher for serving Tonic gRPC and Connect (Axum) on one port.

use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use axum::body::Body as AxumBody;
use axum::response::Response as AxumResponse;
use bytes::Bytes;
use futures::future::BoxFuture;
use http_body::Body as HttpBody;
use http_body_util::BodyExt as _;
use hyper::http::header::CONTENT_TYPE;
use hyper::http::{Request, Response, StatusCode};
use tower::ServiceExt as _;

/// Returns true if the request looks like a gRPC (Tonic) call based on `content-type`.
fn is_grpc(req: &Request<AxumBody>) -> bool {
    req.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map_or(false, |s| s.starts_with("application/grpc"))
}

/// Map any `http_body::Body` into `axum::Body` by streaming bytes.
fn to_axum_body<B>(body: B) -> AxumBody
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    AxumBody::from_stream(body.into_data_stream())
}

fn internal_error<E: std::fmt::Display>(err: E) -> AxumResponse {
    let mut r = AxumResponse::new(AxumBody::from(format!("internal error: {err}")));
    *r.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    r
}

/// A dispatcher that forwards gRPC requests to a Tonic service and others to an Axum router.
///
/// This allows serving both Tonic gRPC and Connect/Axum routes on the same port.
/// Previously named `TonicCompatible`.
#[derive(Clone, Debug)]
pub struct ContentTypeSwitch<G, H> {
    grpc: G,
    http: H,
}

impl<G, H> ContentTypeSwitch<G, H> {
    pub fn new(grpc: G, http: H) -> Self {
        Self { grpc, http }
    }
}

impl<G, GB, H, HB> tower::Service<Request<AxumBody>> for ContentTypeSwitch<G, H>
where
    // Tonic server
    G: tower::Service<Request<AxumBody>, Response = Response<GB>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    GB: HttpBody<Data = Bytes> + Send + 'static,
    GB::Error: std::error::Error + Send + Sync + 'static,
    G::Future: Send + 'static,
    // Axum router
    H: tower::Service<Request<AxumBody>, Response = Response<HB>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    HB: HttpBody<Data = Bytes> + Send + 'static,
    HB::Error: std::error::Error + Send + Sync + 'static,
    H::Future: Send + 'static,
{
    type Response = AxumResponse;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // We clone per-request and use `oneshot`, so we're always ready.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<AxumBody>) -> Self::Future {
        let is_grpc_req = is_grpc(&req);
        let grpc = self.grpc.clone();
        let http = self.http.clone();

        Box::pin(async move {
            if is_grpc_req {
                match tower::ServiceExt::oneshot(grpc, req).await {
                    Ok(res) => {
                        let (parts, body) = res.into_parts();
                        Ok(Response::from_parts(parts, to_axum_body(body)))
                    }
                    Err(e) => Ok(internal_error(e)),
                }
            } else {
                match tower::ServiceExt::oneshot(http, req).await {
                    Ok(res) => {
                        let (parts, body) = res.into_parts();
                        Ok(Response::from_parts(parts, to_axum_body(body)))
                    }
                    Err(e) => Ok(internal_error(e)),
                }
            }
        })
    }
}

/// Backwards name for the content-type dispatcher.
pub type TonicCompatible<G, H> = ContentTypeSwitch<G, H>;
