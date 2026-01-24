pub mod context;
pub mod handler;
pub mod layer;
pub mod message;
pub mod service_builder;
#[cfg(feature = "tonic")]
pub mod tonic;

// Re-export key types at the crate root for convenience
#[cfg(feature = "tonic")]
pub use crate::tonic::{
    BidiStream,
    // Boxed call types
    BoxedBidiStreamCall,
    BoxedCall,
    BoxedClientStreamCall,
    BoxedStream,
    BoxedStreamCall,
    // Parts types
    CapturedParts,
    ClientStream,
    ContentTypeSwitch,
    FromRequestPartsLayer,
    // Factory traits
    IntoBidiStreamFactory,
    IntoClientStreamFactory,
    IntoFactory,
    IntoStreamFactory,
    RequestContext,
    ServerStream,
    TonicCompatible,
    // Unified handler wrapper and RPC markers
    TonicHandlerWrapper,
    Unary,
    // Routing functions
    post_tonic,
    post_tonic_bidi_stream,
    post_tonic_client_stream,
    post_tonic_stream,
    // Unimplemented handlers
    unimplemented_boxed_bidi_stream_call,
    unimplemented_boxed_call,
    unimplemented_boxed_client_stream_call,
    unimplemented_boxed_stream_call,
};
// Re-export from context module
pub use context::{
    BoxedCodec,
    // Compression header constants
    CONNECT_ACCEPT_ENCODING,
    CONNECT_CONTENT_ENCODING,
    CONNECT_TIMEOUT_MS_HEADER,
    // Codec trait and boxed type
    Codec,
    // Compression types
    CompressionConfig,
    CompressionContext,
    CompressionEncoding,
    CompressionLevel,
    ConnectContext,
    ConnectTimeout,
    // Errors
    ContextError,
    // Envelope compression for streaming
    EnvelopeCompression,
    // Idempotency
    IdempotencyLevel,
    // Identity codec (always available)
    IdentityCodec,
    // Limits
    MessageLimits,
    RequestProtocol,
    // Compression functions
    compress_bytes,
    // Timeout
    compute_effective_timeout,
    decompress_bytes,
    // Protocol and context
    detect_protocol,
    negotiate_response_encoding,
    parse_envelope_compression,
    parse_timeout,
    resolve_codec,
};
// Feature-gated codec exports
#[cfg(feature = "compression-br-stream")]
pub use context::BrotliCodec;
#[cfg(feature = "compression-deflate-stream")]
pub use context::DeflateCodec;
#[cfg(feature = "compression-gzip-stream")]
pub use context::GzipCodec;
#[cfg(feature = "compression-zstd-stream")]
pub use context::ZstdCodec;
// Re-export from message module
pub use handler::{ConnectHandler, ConnectHandlerWrapper, get_connect, post_connect};
pub use layer::{BridgeLayer, BridgeService, ConnectLayer, ConnectService};
pub use message::{RequestPipeline, ResponsePipeline};
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
        BoxedCodec,
        // Compression header constants
        CONNECT_ACCEPT_ENCODING,
        CONNECT_CONTENT_ENCODING,
        CONNECT_TIMEOUT_MS_HEADER,
        // Codec trait and boxed type
        Codec,
        CompressionConfig,
        CompressionContext,
        CompressionEncoding,
        CompressionLevel,
        ConnectContext,
        ConnectTimeout,
        // Errors
        ContextError,
        // Compression types
        EnvelopeCompression,
        // Idempotency
        IdempotencyLevel,
        // Identity codec (always available)
        IdentityCodec,
        // Limits
        MessageLimits,
        RequestProtocol,
        // Compression functions
        compress_bytes,
        // Timeout
        compute_effective_timeout,
        decompress_bytes,
        // Protocol and context
        detect_protocol,
        negotiate_response_encoding,
        parse_timeout,
        resolve_codec,
    };
    // Feature-gated codec exports for prelude
    #[cfg(feature = "compression-br-stream")]
    pub use crate::context::BrotliCodec;
    #[cfg(feature = "compression-deflate-stream")]
    pub use crate::context::DeflateCodec;
    #[cfg(feature = "compression-gzip-stream")]
    pub use crate::context::GzipCodec;
    #[cfg(feature = "compression-zstd-stream")]
    pub use crate::context::ZstdCodec;

    pub use crate::handler::{ConnectHandler, ConnectHandlerWrapper, get_connect, post_connect};
    pub use crate::layer::{BridgeLayer, BridgeService, ConnectLayer, ConnectService};
    pub use crate::message::error::{Code, ConnectError, ErrorDetail, Status};
    pub use crate::message::{
        ConnectRequest, ConnectResponse, RequestPipeline, ResponsePipeline, StreamBody, Streaming,
    };
    pub use crate::service_builder::MakeServiceBuilder;
    #[cfg(feature = "tonic")]
    pub use crate::tonic::{
        BidiStream,
        BoxedBidiStreamCall,
        BoxedCall,
        BoxedClientStreamCall,
        BoxedStream,
        BoxedStreamCall,
        CapturedParts,
        ClientStream,
        ContentTypeSwitch,
        FromRequestPartsLayer,
        IntoBidiStreamFactory,
        IntoClientStreamFactory,
        IntoFactory,
        IntoStreamFactory,
        RequestContext,
        ServerStream,
        TonicCompatible,
        // Unified wrapper and markers
        TonicHandlerWrapper,
        Unary,
        // Routing functions
        post_tonic,
        post_tonic_bidi_stream,
        post_tonic_client_stream,
        post_tonic_stream,
        // Unimplemented handlers
        unimplemented_boxed_bidi_stream_call,
        unimplemented_boxed_call,
        unimplemented_boxed_client_stream_call,
        unimplemented_boxed_stream_call,
    };
}
