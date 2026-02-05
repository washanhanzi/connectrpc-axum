use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use connectrpc_axum::prelude::*;
use crate::{EchoRequest, EchoResponse, echo_service_connect};
use futures::StreamExt;

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

async fn echo_client_stream(
    ApiKey(key): ApiKey,
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut messages = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => messages.push(msg.message),
            Err(e) => return Err(e),
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages from {}: [{}]",
            messages.len(),
            key,
            messages.join(", ")
        ),
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
