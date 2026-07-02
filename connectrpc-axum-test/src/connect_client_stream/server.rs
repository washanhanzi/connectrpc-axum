use crate::{EchoRequest, EchoResponse, echo_service_connect};
use connectrpc_axum::prelude::*;
use futures::StreamExt;

async fn echo_client_stream(
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
            "Received {} messages: [{}]",
            messages.len(),
            messages.join(", ")
        ),
    }))
}

fn app() -> axum::Router<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build()
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    axum::serve(listener, app()).await?;
    Ok(())
}

pub async fn start_tcp(listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    axum::serve(listener, app()).await?;
    Ok(())
}
