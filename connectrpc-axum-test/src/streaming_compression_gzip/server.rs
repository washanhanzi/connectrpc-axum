use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use futures::Stream;

async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let response_stream = async_stream::stream! {
        yield Ok(HelloResponse {
            message: format!("Hi {}!", name),
            response_type: None,
        });

        let large_text = format!(
            "Hello {name}! This is a much longer message that should definitely exceed the \
            100 byte compression threshold. Padding: {} {} {}",
            "padding_text ".repeat(10),
            "more_padding ".repeat(10),
            "final_padding ".repeat(10)
        );
        yield Ok(HelloResponse {
            message: large_text,
            response_type: None,
        });

        yield Ok(HelloResponse {
            message: format!(
                "Stream for {}: {} {}",
                name,
                "repeated_content ".repeat(15),
                "more_content ".repeat(15)
            ),
            response_type: None,
        });

        yield Ok(HelloResponse {
            message: format!("Bye {}!", name),
            response_type: None,
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
