use connectrpc_axum::CompressionConfig;
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

async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    let response_stream = async_stream::stream! {
        yield Ok(HelloResponse { message: format!("Hi {}!", name), response_type: None });
        let large = format!("Hello {name}! Padding: {} {} {}",
            "padding_text ".repeat(10), "more_padding ".repeat(10), "final_padding ".repeat(10));
        yield Ok(HelloResponse { message: large, response_type: None });
        yield Ok(HelloResponse {
            message: format!("Stream for {}: {} {}", name,
                "repeated_content ".repeat(15), "more_content ".repeat(15)),
            response_type: None,
        });
        yield Ok(HelloResponse { message: format!("Bye {}!", name), response_type: None });
    };
    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

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
        message: format!("Received {} messages: [{}]", messages.len(), messages.join(", ")),
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let hello_router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .say_hello_stream(say_hello_stream)
        .build();

    let echo_router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(hello_router)
        .add_router(echo_router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
