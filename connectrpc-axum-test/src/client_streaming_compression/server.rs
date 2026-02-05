use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use crate::{EchoRequest, EchoResponse, echo_service_connect};
use futures::StreamExt;

async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut messages = Vec::new();
    let mut stream = streaming.into_stream();

    while let Some(result) = stream.next().await {
        match result {
            Ok(req) => messages.push(req.message),
            Err(e) => return Err(e),
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages: [{}]",
            messages.len(),
            messages.join(", ")
        ),
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
