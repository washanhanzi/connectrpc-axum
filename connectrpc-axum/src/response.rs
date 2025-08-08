//! Response types for Connect.
use crate::error::ConnectError;
use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use prost::Message;
use serde::Serialize;

const APPLICATION_JSON: &str = "application/json";

#[derive(Debug, Clone)]
pub struct ConnectResponse<T>(pub T);

impl<T> ConnectResponse<T> {
    /// Extract the inner value from the ConnectResponse wrapper.
    /// This is useful for converting ConnectResponse back to the original type,
    /// particularly when bridging to Tonic handlers.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> IntoResponse for ConnectResponse<T>
where
    T: Message + Serialize,
{
    fn into_response(self) -> Response {
        // This implementation defaults to JSON. A more complete implementation
        // would check the `Accept` header of the request to decide whether to
        // serialize as Protobuf or JSON. However, `IntoResponse` does not have
        // access to the request headers.
        //
        // The idiomatic Axum way to solve this is to have a custom response
        // type that is constructed with the necessary information from the
        // request, or to use a layer to modify the response based on request
        // properties. For now, we keep it simple and default to JSON.
        let body = serde_json::to_vec(&self.0).unwrap();
        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static(APPLICATION_JSON),
            )
            .body(Body::from(body))
            .unwrap()
    }
}

// So that `Result<ConnectResponse<T>, ConnectError>` can be returned from handlers.
impl<T> From<ConnectResponse<T>> for Result<ConnectResponse<T>, ConnectError> {
    fn from(res: ConnectResponse<T>) -> Self {
        Ok(res)
    }
}

// Note: IntoResponse for Result<ConnectResponse<T>, ConnectError> is implemented by Axum's default implementation
