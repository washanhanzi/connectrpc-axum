use std::sync::Arc;

use anyhow::Context;
use connectrpc_axum_benchmarks::support::{ValkeyPool, load_response};
use connectrpc_axum_benchmarks::{
    GetFortunesRequest, GetFortunesResponse,
    fortune_service_server::{FortuneService, FortuneServiceServer},
};
use tonic::codec::CompressionEncoding;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

const VALKEY_POOL_SIZE: usize = 8;

struct FortuneServiceImpl {
    pool: Arc<ValkeyPool>,
}

#[tonic::async_trait]
impl FortuneService for FortuneServiceImpl {
    async fn get_fortunes(
        &self,
        _request: Request<GetFortunesRequest>,
    ) -> Result<Response<GetFortunesResponse>, Status> {
        let response = load_response(self.pool.as_ref())
            .await
            .map_err(|err| Status::internal(err.to_string()))?;
        Ok(Response::new(response))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let valkey_addr = std::env::args()
        .nth(1)
        .context("usage: tonic_app_server <valkey_addr>")?;
    let pool = Arc::new(ValkeyPool::connect(&valkey_addr, VALKEY_POOL_SIZE).await?);

    let incoming = tonic::transport::server::TcpIncoming::bind("127.0.0.1:0".parse()?)?
        .with_nodelay(Some(true));
    let addr = incoming.local_addr()?;
    println!("{addr}");

    let service = FortuneServiceServer::new(FortuneServiceImpl { pool })
        .accept_compressed(CompressionEncoding::Gzip)
        .send_compressed(CompressionEncoding::Gzip);

    Server::builder()
        .add_service(service)
        .serve_with_incoming_shutdown(incoming, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
