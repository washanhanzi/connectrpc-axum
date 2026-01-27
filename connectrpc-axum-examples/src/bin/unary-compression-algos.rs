use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
// SocketAddr now provided by server_addr()

async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let large_response = format!(
        "Hello, {}! Here is some additional content to ensure the response exceeds the \
        compression threshold. {} {} {}",
        name,
        "padding_text ".repeat(10),
        "more_padding ".repeat(10),
        "final_content ".repeat(10)
    );

    Ok(ConnectResponse::new(HelloResponse {
        message: large_response,
        response_type: None,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Unary Compression (All Algorithms) Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Compression: enabled (min_bytes=100)");
    println!("Supported encodings: gzip, deflate, br, zstd");
    println!("Service: HelloWorldService");
    println!("  - SayHello: POST /hello.HelloWorldService/SayHello");
    println!();
    println!("Test with:");
    println!("  go test -v -run TestUnaryCompression_Deflate");
    println!("  go test -v -run TestUnaryCompression_Brotli");
    println!("  go test -v -run TestUnaryCompression_Zstd");

    axum::serve(listener, app).await?;
    Ok(())
}
