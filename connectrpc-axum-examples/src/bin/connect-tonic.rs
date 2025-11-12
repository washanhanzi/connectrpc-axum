use axum::extract::State;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, helloworldservice};
use futures::Stream;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::AtomicUsize};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

// Handler with state (unary)
async fn say_hello(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state
        .counter
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello #{}, {}!", count, req.name.unwrap_or_default()),
        response_type: None,
    }))
}

// Server streaming handler with state - returns multiple responses!
async fn say_hello_stream(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "Anonymous".to_string());
    let hobbies = req.hobbies;
    let counter = state.counter.clone();

    // Create a stream that yields multiple responses
    let response_stream = async_stream::stream! {
        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("Stream #{}: Hello, {}!", count, name),
            response_type: None,
        });

        if !hobbies.is_empty() {
            for (idx, hobby) in hobbies.iter().enumerate() {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                yield Ok(HelloResponse {
                    message: format!("Stream #{}: Hobby {}: {}", count, idx + 1, hobby),
                    response_type: None,
                });
            }
        } else {
            for i in 1..=3 {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                yield Ok(HelloResponse {
                    message: format!("Stream #{}: Message {} for {}", count, i, name),
                    response_type: None,
                });
            }
        }

        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("Stream #{}: Goodbye, {}!", count, name),
            response_type: None,
        });
    };

    Ok(ConnectResponse(StreamBody::new(response_stream)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // ✅ Now using TonicCompatibleBuilder with streaming support (Phases 3 & 4 complete!)
    // This demonstrates that the tonic-compatible builder now works with streaming methods
    let (connect_router, _grpc_server) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .say_hello_stream(say_hello_stream)
            .with_state(app_state.clone())
            .build();

    // Note: The grpc_server is available for use with tonic::transport::Server
    // For this example, we're just running the Connect server
    let app = connect_router;

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Connect server listening on http://{}", addr);
    println!("Example: TonicCompatibleBuilder with state and server streaming");
    println!("  - Unary RPC: SayHello (with counter)");
    println!("  - Server streaming RPC: SayHelloStream (multiple messages with counter)");
    println!("  - Single handler works for both Connect and gRPC!");

    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
