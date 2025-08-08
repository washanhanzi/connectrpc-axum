use axum::extract::State;
use connectrpc_axum::{error::Code, prelude::*};
use futures::{Stream, stream};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tower::make::Shared;

pub mod hello {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}

// Import generated code - ConnectRPC handlers
use hello::{HelloRequest, HelloResponse, helloworldservice};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

// Individual handler functions that work with ConnectHandler trait
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

async fn say_hello_stream(
    _state: State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> ConnectStreamResponse<Box<dyn Stream<Item = Result<HelloResponse, ConnectError>> + Send + Unpin>>
{
    let stream = stream::iter(vec![
        Ok(HelloResponse {
            message: format!("Hello, {}", req.name.unwrap_or_default()),
        }),
        Ok(HelloResponse {
            message: "Here is a second message.".to_string(),
        }),
        Err(ConnectError::new(
            Code::Unknown,
            "Stream error!".to_string(),
        )),
    ]);
    let boxed_stream: Box<dyn Stream<Item = Result<HelloResponse, ConnectError>> + Send + Unpin> =
        Box::new(stream);
    ConnectStreamResponse::new(boxed_stream)
}

#[tokio::main]
async fn main() {
    let app_state = AppState::default();

    // Create handlers struct with individual handler functions
    let handlers = helloworldservice::HelloWorldServiceHandlers {
        say_hello,
        say_hello_stream,
    };

    // Create ConnectRPC router with handlers struct
    let connect_router = helloworldservice::router(handlers).with_state(app_state.clone());

    // Serve ConnectRPC
    let app = connect_router.fallback(unimplemented);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030")
        .await
        .unwrap();

    println!("listening on http://{:?}", listener.local_addr().unwrap());
    println!("Serving ConnectRPC protocol with flexible ConnectHandler system!");

    axum::serve(listener, Shared::new(app.into_service()))
        .await
        .unwrap();
}

async fn unimplemented() -> ConnectError {
    ConnectError::new(
        Code::Unimplemented,
        "The requested service has not been implemented.",
    )
}
