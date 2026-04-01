use connectrpc_axum_benchmarks::{
    BenchRequest, BenchResponse,
    bench_service_server::{BenchService, BenchServiceServer},
};
use tonic::codec::CompressionEncoding;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

struct BenchServiceImpl;

#[tonic::async_trait]
impl BenchService for BenchServiceImpl {
    async fn unary(
        &self,
        request: Request<BenchRequest>,
    ) -> Result<Response<BenchResponse>, Status> {
        Ok(Response::new(BenchResponse {
            payload: request.into_inner().payload,
        }))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let incoming = tonic::transport::server::TcpIncoming::bind("127.0.0.1:0".parse()?)?
        .with_nodelay(Some(true));
    let addr = incoming.local_addr()?;
    println!("{addr}");

    let service = BenchServiceServer::new(BenchServiceImpl)
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
