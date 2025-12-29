pub mod context;
pub mod error;
pub mod handler;
pub mod layer;
pub mod message;
pub mod service_builder;
#[cfg(feature = "tonic")]
pub mod tonic;

// Re-export key types at the crate root for convenience
#[cfg(feature = "tonic")]
pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
pub use context::{ConnectTimeout, DEFAULT_MAX_MESSAGE_SIZE, MessageLimits, RequestProtocol};
#[cfg(feature = "tonic")]
pub use handler::{
    BoxedCall, IntoFactory, TonicCompatibleHandlerWrapper, post_connect_tonic,
    unimplemented_boxed_call,
};
pub use handler::{
    ConnectBidiStreamHandler, ConnectBidiStreamHandlerWrapper, ConnectClientStreamHandler,
    ConnectClientStreamHandlerWrapper, ConnectHandler, ConnectHandlerWrapper, post_connect,
    post_connect_bidi_stream, post_connect_client_stream,
};
pub use layer::{ConnectLayer, ConnectService};
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
    pub use crate::context::{
        ConnectTimeout, DEFAULT_MAX_MESSAGE_SIZE, MessageLimits, RequestProtocol,
    };
    pub use crate::error::{Code, ConnectError};
    #[cfg(feature = "tonic")]
    pub use crate::handler::{
        BoxedCall, IntoFactory, TonicCompatibleHandlerWrapper, post_connect_tonic,
        unimplemented_boxed_call,
    };
    pub use crate::handler::{
        ConnectBidiStreamHandler, ConnectBidiStreamHandlerWrapper, ConnectClientStreamHandler,
        ConnectClientStreamHandlerWrapper, ConnectHandler, ConnectHandlerWrapper,
        ConnectStreamHandlerWrapper, post_connect, post_connect_bidi_stream,
        post_connect_client_stream, post_connect_stream,
    };
    pub use crate::layer::{ConnectLayer, ConnectService};
    pub use crate::message::{ConnectRequest, ConnectResponse, ConnectStreamResponse, ConnectStreamingRequest, StreamBody};
    pub use crate::service_builder::MakeServiceBuilder;
    #[cfg(feature = "tonic")]
    pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
}
