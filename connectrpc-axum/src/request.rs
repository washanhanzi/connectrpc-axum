//! Extractor for Connect requests.
use crate::error::Code;
use crate::error::ConnectError;
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{Method, header},
};
use prost::Message;
use serde::Deserialize;
use serde::de::DeserializeOwned;

const APPLICATION_PROTO: &str = "application/proto";
const APPLICATION_JSON: &str = "application/json";

#[derive(Debug, Clone)]
pub struct ConnectRequest<T>(pub T);

impl<S, T> FromRequest<S> for ConnectRequest<T>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    type Rejection = ConnectError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match *req.method() {
            Method::POST => from_post_request(req, state).await,
            Method::GET => from_get_request(req, state).await,
            _ => Err(ConnectError::new(
                Code::Unimplemented,
                "HTTP method not supported".to_string(),
            )),
        }
    }
}

async fn from_post_request<S, T>(
    req: Request,
    _state: &S,
) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let content_type = content_type.to_string();

    let bytes = Bytes::from_request(req, _state)
        .await
        .map_err(|err| ConnectError::new(Code::Internal, err.to_string()))?;

    if content_type.starts_with(APPLICATION_PROTO) {
        let message = T::decode(bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;
        Ok(ConnectRequest(message))
    } else if content_type.starts_with(APPLICATION_JSON) {
        let message: T = serde_json::from_slice(&bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;
        Ok(ConnectRequest(message))
    } else {
        Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported content-type",
        ))
    }
}

#[derive(Deserialize)]
struct GetRequestQuery {
    connect: String,
    encoding: String,
    message: String,
    base64: Option<String>,
    #[allow(dead_code)] // Not used yet, but part of the spec
    compression: Option<String>,
}

async fn from_get_request<S, T>(req: Request, _state: &S) -> Result<ConnectRequest<T>, ConnectError>
where
    S: Send + Sync,
    T: Message + DeserializeOwned + Default,
{
    let query = req.uri().query().unwrap_or("");
    let params: GetRequestQuery = serde_qs::from_str(query)
        .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?;

    if params.connect != "v1" {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported connect version",
        ));
    }

    let bytes = if params.base64.as_deref() == Some("true") {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::URL_SAFE
            .decode(&params.message)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        params.message.into_bytes()
    };

    let message = if params.encoding == "proto" {
        T::decode(bytes.as_slice())
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else if params.encoding == "json" {
        serde_json::from_slice(&bytes)
            .map_err(|err| ConnectError::new(Code::InvalidArgument, err.to_string()))?
    } else {
        return Err(ConnectError::new(
            Code::InvalidArgument,
            "unsupported encoding",
        ));
    };

    Ok(ConnectRequest(message))
}
