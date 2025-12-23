pub mod error;
pub mod handler;
pub mod layer;
pub mod protocol;
pub mod request;
pub mod response;
pub mod service_builder;
pub mod stream_response;
#[cfg(feature = "tonic")]
pub mod tonic;

// Re-export key types at the crate root for convenience
#[cfg(feature = "tonic")]
pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
#[cfg(feature = "tonic")]
pub use handler::{
    BoxedCall, IntoFactory, TonicCompatibleHandlerWrapper, post_connect_tonic,
    unimplemented_boxed_call,
};
pub use handler::{ConnectHandler, ConnectHandlerWrapper, post_connect};
pub use layer::{ConnectLayer, ConnectService};
pub use protocol::{RequestProtocol, get_request_protocol};
pub use service_builder::MakeServiceBuilder;

// Re-export several crates
pub use futures;
pub use pbjson;
pub use pbjson_types;
pub use prost;
pub use serde;

pub use prelude::*;

pub mod prelude {
    //! A prelude for `axum-connect` providing the most common types.
    pub use crate::error::{Code, ConnectError};
    #[cfg(feature = "tonic")]
    pub use crate::handler::{
        BoxedCall, IntoFactory, TonicCompatibleHandlerWrapper, post_connect_tonic,
        unimplemented_boxed_call,
    };
    pub use crate::handler::{
        ConnectHandler, ConnectHandlerWrapper, ConnectStreamHandlerWrapper, post_connect,
        post_connect_stream,
    };
    pub use crate::layer::{ConnectLayer, ConnectService};
    pub use crate::protocol::{RequestProtocol, get_request_protocol};
    pub use crate::request::ConnectRequest;
    pub use crate::response::{ConnectResponse, StreamBody};
    pub use crate::service_builder::MakeServiceBuilder;
    pub use crate::stream_response::ConnectStreamResponse;
    #[cfg(feature = "tonic")]
    pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
}
