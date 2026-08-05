use axum::http::HeaderMap;
use connectrpc_axum::prelude::{ConnectError, ConnectRequest, ConnectResponse};
use connectrpc_axum_examples::{
    EchoRequest, EchoResponse, echo_service_connect, echo_service_connect_client::EchoServiceClient,
};

async fn echo_user_id(
    headers: HeaderMap,
    ConnectRequest(request): ConnectRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let user_id = headers
        .get("x-user-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");

    Ok(ConnectResponse::new(EchoResponse {
        message: format!("{}:{user_id}", request.message),
    }))
}

#[tokio::test]
async fn generated_client_sends_default_header_to_unary_handler() -> anyhow::Result<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo(echo_user_id)
        .build_connect();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let test_result = async {
        let client = EchoServiceClient::builder(format!("http://{address}"))
            .try_default_header("x-user-id", "user-123")?
            .build()?;
        let response = client
            .echo(&EchoRequest {
                message: "hello".to_owned(),
            })
            .await?;

        assert_eq!(response.message, "hello:user-123");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    let _ = shutdown_tx.send(());
    server.await??;
    test_result
}
