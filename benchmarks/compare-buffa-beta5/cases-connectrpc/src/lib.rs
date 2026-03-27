include!(concat!(env!("OUT_DIR"), "/protos.rs"));

use axum::Router;
use buffa::view::OwnedView;
use compare_buffa_beta5_common::{VALKEY_POOL_SIZE, ValkeyPool, query_fortunes};
use connectrpc::{ConnectError, ConnectRpcService, Context};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

use crate::fortune::v1::{
    Fortune, FortuneService, FortuneServiceServer, GetFortunesRequestView, GetFortunesResponse,
};

struct FortuneServiceImpl {
    pool: Arc<ValkeyPool>,
}

pub struct NativeBenchmarkServer {
    pub base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for NativeBenchmarkServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

impl FortuneService for FortuneServiceImpl {
    async fn get_fortunes(
        &self,
        ctx: Context,
        _request: OwnedView<GetFortunesRequestView<'static>>,
    ) -> Result<(GetFortunesResponse, Context), ConnectError> {
        let mut conn = self.pool.get();
        let fortunes = query_fortunes(&mut conn)
            .await
            .map_err(|error| ConnectError::internal(format!("valkey: {error}")))?;

        Ok((
            GetFortunesResponse {
                fortunes: fortunes
                    .into_iter()
                    .map(|(id, message)| Fortune {
                        id,
                        message,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            ctx,
        ))
    }
}

pub async fn connect_app(valkey_addr: &str) -> Router {
    let pool = Arc::new(
        ValkeyPool::connect(valkey_addr, VALKEY_POOL_SIZE)
            .await
            .expect("connect connect-rust valkey pool"),
    );

    let service = FortuneServiceServer::new(FortuneServiceImpl { pool });
    Router::new().fallback_service(ConnectRpcService::new(service))
}

pub async fn spawn_native_server(valkey_addr: &str) -> NativeBenchmarkServer {
    let pool = Arc::new(
        ValkeyPool::connect(valkey_addr, VALKEY_POOL_SIZE)
            .await
            .expect("connect connect-rust valkey pool"),
    );

    let service = ConnectRpcService::new(FortuneServiceServer::new(FortuneServiceImpl { pool }));
    let bound = connectrpc::server::Server::bind("127.0.0.1:0")
        .await
        .expect("bind connect-rust native benchmark server");
    let addr = bound
        .local_addr()
        .expect("read connect-rust native benchmark address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        tokio::select! {
            result = bound.serve_with_service(service) => {
                result.expect("run connect-rust native benchmark server");
            }
            _ = shutdown_rx => {}
        }
    });

    sleep(Duration::from_millis(25)).await;

    NativeBenchmarkServer {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
    }
}
