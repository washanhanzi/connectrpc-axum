use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect, EchoRequest, EchoResponse, echo_service_connect};
use futures::{Stream, StreamExt};

async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}

async fn echo_bidi_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let mut stream = streaming.into_stream();

    let response_stream = async_stream::stream! {
        let mut count = 0;
        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    count += 1;
                    yield Ok(EchoResponse {
                        message: format!("Echo #{}: {}", count, msg.message),
                    });
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut messages = Vec::new();

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
    let (hello_connect, hello_grpc) =
        hello_world_service_connect::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .build();

    let (echo_connect, echo_grpc) =
        echo_service_connect::EchoServiceTonicCompatibleBuilder::new()
            .echo_bidi_stream(echo_bidi_stream)
            .echo_client_stream(echo_client_stream)
            .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_connect)
        .add_router(echo_connect)
        .add_grpc_service(hello_grpc)
        .add_grpc_service(echo_grpc)
        .build();

    axum::serve(listener, tower::make::Shared::new(app)).await?;
    Ok(())
}
