use connectrpc_axum::prelude::*;
use futures::Stream;
use futures::StreamExt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, atomic::AtomicUsize};
use axum::extract::State;
use tonic::Status;
use connectrpc_axum_examples::{
    HelloRequest, HelloResponse, helloworldservice,
    EchoRequest, EchoResponse, echo_service_server,
};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

// ============================================================================
// Hello Service - Connect Handlers
// ============================================================================

async fn say_hello(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello #{}, {}!", count, req.name.unwrap_or_default()),
    }))
}

async fn say_hello_stream(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "Anonymous".to_string());
    let counter = state.counter.clone();

    let response_stream = async_stream::stream! {
        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("Stream #{}: Hello, {}!", count, name),
        });

        for i in 1..=3 {
            let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            yield Ok(HelloResponse {
                message: format!("Stream #{}: Message {} for {}", count, i, name),
            });
        }

        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        yield Ok(HelloResponse {
            message: format!("Stream #{}: Goodbye, {}!", count, name),
        });
    };

    Ok(ConnectResponse(StreamBody::new(response_stream)))
}

// ============================================================================
// Echo Service - Custom Tonic Implementation with State
// ============================================================================

struct EchoServiceImpl {
    app_state: AppState,
}

#[tonic::async_trait]
impl echo_service_server::EchoService for EchoServiceImpl {
    // Unary RPC
    async fn echo(
        &self,
        request: tonic::Request<EchoRequest>,
    ) -> Result<tonic::Response<EchoResponse>, Status> {
        let req = request.into_inner();
        let count = self.app_state.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(tonic::Response::new(EchoResponse {
            message: format!("Echo #{}: {}", count, req.message),
        }))
    }

    // Client streaming RPC
    async fn echo_client_stream(
        &self,
        request: tonic::Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<tonic::Response<EchoResponse>, Status> {
        let mut req_stream = request.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = req_stream.next().await {
            match result {
                Ok(req) => messages.push(req.message),
                Err(e) => return Err(e),
            }
        }

        let count = self.app_state.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(tonic::Response::new(EchoResponse {
            message: format!(
                "Client Stream #{}: Received {} messages: [{}]",
                count,
                messages.len(),
                messages.join(", ")
            ),
        }))
    }

    // Bidirectional streaming RPC
    type EchoBidiStreamStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

    async fn echo_bidi_stream(
        &self,
        request: tonic::Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<tonic::Response<Self::EchoBidiStreamStream>, Status> {
        let mut req_stream = request.into_inner();
        let counter = self.app_state.counter.clone();

        let response_stream = async_stream::stream! {
            let mut message_count = 0;
            while let Some(result) = req_stream.next().await {
                match result {
                    Ok(request) => {
                        message_count += 1;
                        let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        yield Ok(EchoResponse {
                            message: format!(
                                "Bidi Echo #{} (msg #{}): {}",
                                count, message_count, request.message
                            ),
                        });
                    }
                    Err(e) => {
                        yield Err(e);
                        break;
                    }
                }
            }

            let final_count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            yield Ok(EchoResponse {
                message: format!(
                    "Bidi stream #{} completed. Received {} messages total.",
                    final_count, message_count
                ),
            });
        };

        Ok(tonic::Response::new(Box::pin(response_stream)))
    }
}

// ============================================================================
// Main Server
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    // Build Connect router for HelloWorldService
    let hello_router = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .say_hello_stream(say_hello_stream)
        .with_state(app_state.clone())
        .build();

    // Build custom Tonic gRPC service for EchoService
    let echo_grpc_service =
        echo_service_server::EchoServiceServer::new(EchoServiceImpl {
            app_state: app_state.clone(),
        });

    // Use MakeServiceBuilder to combine multiple services
    // - HelloWorldService via Connect router (unary + server streaming)
    // - EchoService via Tonic gRPC (unary + client streaming + bidi streaming)
    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_router)
        .add_grpc_service(echo_grpc_service)
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Multi-service server listening on http://{}", addr);
    println!("Example: Combines Connect (hello.proto) and Tonic (echo.proto) services");
    println!("  - HelloWorldService: Connect router with state");
    println!("  - EchoService: Custom Tonic implementation with bidi streaming");

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
