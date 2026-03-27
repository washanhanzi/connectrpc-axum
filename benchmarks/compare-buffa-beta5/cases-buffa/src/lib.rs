include!(concat!(env!("OUT_DIR"), "/protos.rs"));

use axum::{Router, extract::State};
use buffa::Message;
use compare_buffa_beta5_common::{VALKEY_POOL_SIZE, ValkeyPool, query_fortunes};
use connectrpc_axum::prelude::*;
use std::sync::Arc;

use crate::fortune::v1::{
    Fortune, GetFortunesRequest, GetFortunesResponse, fortune_service_connect,
};

pub const FORTUNE_PATH: &str = "/fortune.v1.FortuneService/GetFortunes";

#[derive(Clone)]
struct AppState {
    pool: Arc<ValkeyPool>,
}

pub fn encode_get_fortunes_request_proto() -> Vec<u8> {
    GetFortunesRequest::default().encode_to_vec()
}

pub fn decode_get_fortunes_response_proto(bytes: &[u8]) -> GetFortunesResponse {
    GetFortunesResponse::decode_from_slice(bytes).expect("decode GetFortunesResponse")
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
            .map(|(id, message)| Fortune {
                id,
                message,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }))
}

pub async fn connect_app(valkey_addr: &str) -> Router {
    let state = AppState {
        pool: Arc::new(
            ValkeyPool::connect(valkey_addr, VALKEY_POOL_SIZE)
                .await
                .expect("connect buffa valkey pool"),
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
