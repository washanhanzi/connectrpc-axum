include!(concat!(env!("OUT_DIR"), "/protos.rs"));

use axum::{Router, extract::State};
use compare_buffa_beta5_common::{VALKEY_POOL_SIZE, ValkeyPool, query_fortunes};
use connectrpc_axum::prelude::*;
use prost::Message;
use std::sync::Arc;

use crate::fortune::v1::{
    Fortune, GetFortunesRequest, GetFortunesResponse, fortune_service_connect,
};

#[derive(Clone)]
struct AppState {
    pool: Arc<ValkeyPool>,
}

pub async fn connect_app(valkey_addr: &str) -> Router {
    let state = AppState {
        pool: Arc::new(
            ValkeyPool::connect(valkey_addr, VALKEY_POOL_SIZE)
                .await
                .expect("connect release valkey pool"),
        ),
    };

    let router = fortune_service_connect::FortuneServiceBuilder::new()
        .get_fortunes(get_fortunes)
        .with_state(state)
        .build();

    connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build()
}

async fn get_fortunes(
    State(state): State<AppState>,
    ConnectRequest(_): ConnectRequest<GetFortunesRequest>,
) -> Result<ConnectResponse<GetFortunesResponse>, ConnectError> {
    let mut conn = state.pool.get();
    let fortunes = query_fortunes(&mut conn)
        .await
        .map_err(|error| ConnectError::new_internal(format!("valkey: {error}")))?;

    Ok(ConnectResponse::new(GetFortunesResponse {
        fortunes: fortunes
            .into_iter()
            .map(|(id, message)| Fortune { id, message })
            .collect(),
    }))
}

pub fn decode_get_fortunes_response_proto(bytes: &[u8]) -> GetFortunesResponse {
    GetFortunesResponse::decode(bytes).expect("decode GetFortunesResponse")
}
