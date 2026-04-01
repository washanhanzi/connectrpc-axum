use std::sync::Arc;

use anyhow::Context;
use buffa::view::OwnedView;
use connectrpc::{ConnectError, ConnectRpcService, Context as ConnectContext};
use connectrpc_axum_benchmarks::connect_rust::generated::fortune::v1::{
    Fortune, FortuneService, FortuneServiceServer, GetFortunesRequestView, GetFortunesResponse,
};
use connectrpc_axum_benchmarks::support::{ValkeyPool, query_fortunes};

const VALKEY_POOL_SIZE: usize = 8;

struct FortuneServiceImpl {
    pool: Arc<ValkeyPool>,
}

impl FortuneService for FortuneServiceImpl {
    async fn get_fortunes(
        &self,
        ctx: ConnectContext,
        _request: OwnedView<GetFortunesRequestView<'static>>,
    ) -> Result<(GetFortunesResponse, ConnectContext), ConnectError> {
        let mut conn = self.pool.get();
        let fortunes = query_fortunes(&mut conn)
            .await
            .map_err(|err| ConnectError::internal(format!("valkey: {err}")))?;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let valkey_addr = std::env::args()
        .nth(1)
        .context("usage: connect_rust_app_server <valkey_addr>")?;
    let pool = Arc::new(ValkeyPool::connect(&valkey_addr, VALKEY_POOL_SIZE).await?);

    let server = FortuneServiceServer::new(FortuneServiceImpl { pool });
    let service = ConnectRpcService::new(server);

    let bound = connectrpc::server::Server::bind("127.0.0.1:0")
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let addr = bound.local_addr()?;
    println!("{addr}");

    tokio::select! {
        result = bound.serve_with_service(service) => {
            result.map_err(|err| anyhow::anyhow!(err.to_string()))?
        }
        _ = tokio::signal::ctrl_c() => {}
    }

    Ok(())
}
