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
pub use crate::tonic::{
    ContentTypeSwitch, TonicCompatible,
    // Handler types from tonic module
    post_tonic_unary, post_tonic_stream, post_tonic_client_stream, post_tonic_bidi_stream,
    unimplemented_boxed_call, unimplemented_boxed_stream_call,
    unimplemented_boxed_client_stream_call, unimplemented_boxed_bidi_stream_call,
    BoxedCall, BoxedStreamCall, BoxedClientStreamCall, BoxedBidiStreamCall, BoxedStream,
    IntoFactory, IntoStreamFactory, IntoClientStreamFactory, IntoBidiStreamFactory,
    TonicCompatibleHandlerWrapper, TonicCompatibleStreamHandlerWrapper,
    TonicCompatibleClientStreamHandlerWrapper, TonicCompatibleBidiStreamHandlerWrapper,
    // Parts types
    RequestContext, CapturedParts, FromRequestPartsLayer,
};
// Re-export from context module
pub use context::{
    compress, compute_effective_timeout, decompress, default_codec, detect_protocol,
    negotiate_response_encoding, parse_timeout, Codec, Compression, CompressionConfig,
    CompressionContext, CompressionEncoding, ConnectTimeout, Context, GzipCodec, IdentityCodec,
    MessageLimits, RequestProtocol, ContextError, ServerConfig, CONNECT_TIMEOUT_MS_HEADER,
    DEFAULT_MAX_MESSAGE_SIZE,
};
// Re-export from pipeline module
pub use pipeline::{RequestPipeline, ResponsePipeline};
pub use handler::{
    get_unary, post_bidi_stream, post_client_stream, post_connect, post_server_stream, post_unary,
    ConnectBidiStreamHandler, ConnectBidiStreamHandlerWrapper, ConnectClientStreamHandler,
    ConnectClientStreamHandlerWrapper, ConnectHandler, ConnectHandlerWrapper,
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
        compress, compute_effective_timeout, decompress, default_codec, detect_protocol,
        negotiate_response_encoding, parse_timeout, Codec, Compression, CompressionConfig,
        CompressionContext, CompressionEncoding, ConnectTimeout, Context, GzipCodec,
        IdentityCodec, MessageLimits, RequestProtocol, ContextError, ServerConfig,
        CONNECT_TIMEOUT_MS_HEADER, DEFAULT_MAX_MESSAGE_SIZE,
    };
    pub use crate::pipeline::{RequestPipeline, ResponsePipeline};
    pub use crate::error::{Code, ConnectError, ErrorDetail};
    #[cfg(feature = "tonic")]
    pub use crate::tonic::{
        post_tonic_unary, post_tonic_stream, post_tonic_client_stream, post_tonic_bidi_stream,
        unimplemented_boxed_call, unimplemented_boxed_stream_call,
        unimplemented_boxed_client_stream_call, unimplemented_boxed_bidi_stream_call,
        BoxedCall, BoxedStreamCall, BoxedClientStreamCall, BoxedBidiStreamCall, BoxedStream,
        IntoFactory, IntoStreamFactory, IntoClientStreamFactory, IntoBidiStreamFactory,
        TonicCompatibleHandlerWrapper, TonicCompatibleStreamHandlerWrapper,
        TonicCompatibleClientStreamHandlerWrapper, TonicCompatibleBidiStreamHandlerWrapper,
        ContentTypeSwitch, TonicCompatible,
        RequestContext, CapturedParts, FromRequestPartsLayer,
    };
    pub use crate::handler::{
        get_unary, post_bidi_stream, post_client_stream, post_connect, post_server_stream, post_unary,
        ConnectBidiStreamHandler, ConnectBidiStreamHandlerWrapper, ConnectClientStreamHandler,
        ConnectClientStreamHandlerWrapper, ConnectHandler, ConnectHandlerWrapper,
        ConnectStreamHandlerWrapper,
    };
    pub use crate::layer::{ConnectLayer, ConnectService};
    pub use crate::message::{
        ConnectRequest, ConnectResponse, ConnectStreamResponse, ConnectStreamingRequest, StreamBody,
        Streaming,
    };
    pub use crate::service_builder::MakeServiceBuilder;
}
