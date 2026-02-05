use axum::{extract::FromRequestParts, http::request::Parts};
use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use futures::Stream;

/// Custom extractor that validates the x-api-key header.
/// Returns `ConnectError` on rejection, which gets encoded with Connect protocol.
pub struct ApiKey(pub String);

impl<S> FromRequestParts<S> for ApiKey
where
    S: Send + Sync,
{
    type Rejection = ConnectError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| ApiKey(s.to_string()))
            .ok_or_else(|| ConnectError::new(Code::Unauthenticated, "missing api key"))
    }
}

async fn say_hello_stream(
    ApiKey(_key): ApiKey,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let stream = async_stream::stream! {
        yield Ok(HelloResponse {
            message: format!("Hello, {}!", name),
            response_type: None,
        });
        yield Ok(HelloResponse {
            message: format!("How are you, {}?", name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(stream)))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
