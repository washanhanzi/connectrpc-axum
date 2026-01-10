//! Message types for Connect RPC request and response handling.

mod request;
mod response;

pub use request::{ConnectRequest, Streaming};
pub use response::{ConnectResponse, StreamBody};
