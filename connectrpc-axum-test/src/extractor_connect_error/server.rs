use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};

/// Custom extractor that validates the x-user-id header.
/// Returns `ConnectError` on rejection, which gets encoded with Connect protocol.
pub struct UserId(pub String);

impl<S> FromRequestParts<S> for UserId
where
    S: Send + Sync,
{
    type Rejection = ConnectError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| UserId(s.to_string()))
            .ok_or_else(|| ConnectError::new_unauthenticated("missing x-user-id header"))
    }
}

async fn say_hello(
    UserId(user_id): UserId,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! (authenticated as {})", name, user_id),
        response_type: None,
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
