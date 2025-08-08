pub mod error;
pub mod handler;
pub mod request;
pub mod response;
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
    pub use crate::handler::{ConnectHandler, ConnectHandlerWrapper, post_connect};
    pub use crate::request::ConnectRequest;
    pub use crate::response::ConnectResponse;
    pub use crate::stream_response::ConnectStreamResponse;
    #[cfg(feature = "tonic")]
    pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
}
