//! Message types for Connect RPC request and response handling.

mod request;
mod response;

pub use request::{ConnectRequest, ConnectStreamingRequest, Streaming};
pub use response::{ConnectResponse, StreamBody};
