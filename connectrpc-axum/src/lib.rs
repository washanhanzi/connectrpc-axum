pub mod context;
pub mod error;
pub mod handler;
pub mod layer;
pub mod message;
pub mod pipeline;
pub mod service_builder;
#[cfg(feature = "tonic")]
pub mod tonic;

// Re-export key types at the crate root for convenience
#[cfg(feature = "tonic")]
pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
// Re-export from context module
pub use context::{
    compress, compute_effective_timeout, decompress, detect_protocol, negotiate_response_encoding,
    parse_timeout, Compression, CompressionConfig, CompressionContext, CompressionEncoding,
    ConnectTimeout, Context, MessageLimits, RequestProtocol, ContextError, ServerConfig,
    CONNECT_TIMEOUT_MS_HEADER, DEFAULT_MAX_MESSAGE_SIZE,
};
// Re-export from pipeline module
pub use pipeline::{RequestPipeline, ResponsePipeline};
#[cfg(feature = "tonic")]
pub use handler::{
    post_connect_tonic, unimplemented_boxed_call, BoxedCall, IntoFactory,
    TonicCompatibleHandlerWrapper,
};
pub use handler::{
    post_connect, post_connect_bidi_stream, post_connect_client_stream, ConnectBidiStreamHandler,
    ConnectBidiStreamHandlerWrapper, ConnectClientStreamHandler, ConnectClientStreamHandlerWrapper,
    ConnectHandler, ConnectHandlerWrapper,
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
        compress, compute_effective_timeout, decompress, detect_protocol,
        negotiate_response_encoding, parse_timeout, Compression, CompressionConfig,
        CompressionContext, CompressionEncoding, ConnectTimeout, Context,
        MessageLimits, RequestProtocol, ContextError, ServerConfig, CONNECT_TIMEOUT_MS_HEADER,
        DEFAULT_MAX_MESSAGE_SIZE,
    };
    pub use crate::pipeline::{RequestPipeline, ResponsePipeline};
    pub use crate::error::{Code, ConnectError};
    #[cfg(feature = "tonic")]
    pub use crate::handler::{
        post_connect_tonic, unimplemented_boxed_call, BoxedCall, IntoFactory,
        TonicCompatibleHandlerWrapper,
    };
    pub use crate::handler::{
        post_connect, post_connect_bidi_stream, post_connect_client_stream,
        post_connect_stream, ConnectBidiStreamHandler, ConnectBidiStreamHandlerWrapper,
        ConnectClientStreamHandler, ConnectClientStreamHandlerWrapper, ConnectHandler,
        ConnectHandlerWrapper, ConnectStreamHandlerWrapper,
    };
    pub use crate::layer::{ConnectLayer, ConnectService};
    pub use crate::message::{
        ConnectRequest, ConnectResponse, ConnectStreamResponse, ConnectStreamingRequest, StreamBody,
    };
    pub use crate::service_builder::MakeServiceBuilder;
    #[cfg(feature = "tonic")]
    pub use crate::tonic::{ContentTypeSwitch, TonicCompatible};
}
