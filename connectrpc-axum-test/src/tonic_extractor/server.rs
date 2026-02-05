use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};

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

async fn say_hello(
    ApiKey(key): ApiKey,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! (key: {})", name, key),
        response_type: None,
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let (connect_router, grpc_server) =
        hello_world_service_connect::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    axum::serve(listener, tower::make::Shared::new(app)).await?;
    Ok(())
}
