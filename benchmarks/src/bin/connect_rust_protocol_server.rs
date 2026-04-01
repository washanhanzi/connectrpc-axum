use buffa::MessageField;
use buffa::view::OwnedView;
use connectrpc::{ConnectError, ConnectRpcService, Context as ConnectContext};
use connectrpc_axum_benchmarks::connect_rust::generated::bench::v1::{
    BenchRequestView, BenchResponse, BenchService, BenchServiceServer,
};

struct BenchServiceImpl;

impl BenchService for BenchServiceImpl {
    async fn unary(
        &self,
        ctx: ConnectContext,
        request: OwnedView<BenchRequestView<'static>>,
    ) -> Result<(BenchResponse, ConnectContext), ConnectError> {
        let request = request.to_owned_message();
        Ok((
            BenchResponse {
                payload: match request.payload.as_option() {
                    Some(payload) => MessageField::some(payload.clone()),
                    None => MessageField::none(),
                },
                ..Default::default()
            },
            ctx,
        ))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).is_some() {
        anyhow::bail!("usage: connect_rust_protocol_server");
    }

    let server = BenchServiceServer::new(BenchServiceImpl);
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
