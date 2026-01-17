use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use futures::Stream;
use std::net::SocketAddr;

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
            100 byte compression threshold. We're including lots of repeated text to ensure \
            good compression: {} {} {} {}",
            "padding_text_for_compression ".repeat(5),
            "more_padding_text ".repeat(5),
            "even_more_text ".repeat(5),
            "final_padding ".repeat(5)
        );
        yield Ok(HelloResponse {
            message: large_text,
            response_type: None,
        });

        yield Ok(HelloResponse {
            message: format!(
                "Stream message for {}: {} {} {}",
                name,
                "repeated_content_for_compression ".repeat(10),
                "more_repeated_content ".repeat(10),
                "final_content ".repeat(10)
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Streaming Compression (All Algorithms) Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Compression: enabled (min_bytes=100)");
    println!("Supported encodings: gzip, deflate, br, zstd");
    println!("Service: HelloWorldService");
    println!("  - SayHelloStream: POST /hello.HelloWorldService/SayHelloStream");
    println!();
    println!("Test with:");
    println!("  go test -v -run TestStreamingResponseCompression_Deflate");
    println!("  go test -v -run TestStreamingResponseCompression_Brotli");
    println!("  go test -v -run TestStreamingResponseCompression_Zstd");

    axum::serve(listener, tower::make::Shared::new(app)).await?;
    Ok(())
}
