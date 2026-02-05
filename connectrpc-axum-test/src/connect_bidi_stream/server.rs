use connectrpc_axum::prelude::*;
use crate::{EchoRequest, EchoResponse, echo_service_connect};
use futures::{Stream, StreamExt};

async fn echo_bidi_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<EchoResponse, ConnectError>>>>,
    ConnectError,
> {
    let mut stream = streaming.into_stream();

    let response_stream = async_stream::stream! {
        let mut count = 0;
        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    count += 1;
                    yield Ok(EchoResponse {
                        message: format!("Echo #{}: {}", count, msg.message),
                    });
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
        yield Ok(EchoResponse {
            message: format!("Stream complete. Echoed {} messages.", count),
        });
    };

    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_bidi_stream(echo_bidi_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}
