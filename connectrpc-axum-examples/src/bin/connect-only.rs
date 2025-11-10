use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use futures::Stream;
use std::net::SocketAddr;

// Simple stateless handler for SayHello (unary)
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

// Server streaming handler - returns multiple responses!
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "Anonymous".to_string());
    let hobbies = req.hobbies;

    // Create a stream that yields multiple responses
    let response_stream = async_stream::stream! {
        // First greeting
        yield Ok(HelloResponse {
            message: format!("Hello, {}! Starting stream...", name),
        });

        // If there are hobbies, greet each one
        if !hobbies.is_empty() {
            for (idx, hobby) in hobbies.iter().enumerate() {
                yield Ok(HelloResponse {
                    message: format!("Hobby #{}: {} - that's interesting!", idx + 1, hobby),
                });
            }
        } else {
            // Send multiple greetings if no hobbies provided
            for i in 1..=3 {
                yield Ok(HelloResponse {
                    message: format!("Stream message #{} for {}", i, name),
                });
            }
        }

        // Final message
        yield Ok(HelloResponse {
            message: format!("Stream complete for {}. Goodbye!", name),
        });
    };

    Ok(ConnectResponse(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build Connect router with simple stateless handlers
    let connect_router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .say_hello_stream(say_hello_stream)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Connect-only server listening on http://{}", addr);
    println!("Example: Pure ConnectRPC implementation");
    println!("  - Unary RPC: SayHello");
    println!("  - Server streaming RPC: SayHelloStream (returns multiple messages!)");

    axum::serve(listener, connect_router.into_make_service()).await?;
    Ok(())
}
