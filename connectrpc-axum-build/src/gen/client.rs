//! Client code generation for Connect RPC services.

use convert_case::{Case, Casing};
use prost_build::Service;
use quote::{format_ident, quote};

/// Method information tuple for code generation.
pub type MethodInfo = (
    proc_macro2::Ident,       // method_name
    proc_macro2::TokenStream, // request_type
    proc_macro2::TokenStream, // response_type
    String,                   // path
    proc_macro2::Ident,       // stream_assoc
    bool,                     // is_server_streaming
    bool,                     // is_client_streaming
    Option<i32>,              // idempotency_level
    proc_macro2::TokenStream, // idempotency_tokens
);

/// Generate the Connect RPC client code.
pub fn generate_connect_client(
    service: &Service,
    method_info: &[MethodInfo],
) -> proc_macro2::TokenStream {
    // Service name constant (e.g., "hello.HelloWorldService")
    let service_name_const = format_ident!(
        "{}_SERVICE_NAME",
        service.name.to_case(Case::UpperSnake)
    );
    let full_service_name = format!("{}.{}", service.package, service.proto_name);

    // Procedures module name (e.g., hello_world_service_procedures)
    let procedures_mod_name = format_ident!(
        "{}_procedures",
        service.name.to_case(Case::Snake)
    );

    // Generate procedure constants
    let procedure_constants: Vec<_> = method_info
        .iter()
        .map(|(method_name, _, _, path, _, _, _, _, _)| {
            let const_name = format_ident!("{}", method_name.to_string().to_uppercase());
            quote! {
                /// Full procedure path for this RPC method.
                pub const #const_name: &str = #path;
            }
        })
        .collect();

    // Client struct name (e.g., HelloWorldServiceClient)
    let client_name = format_ident!("{}Client", service.name);
    let client_builder_name = format_ident!("{}ClientBuilder", service.name);

    // Generate typed client methods for all RPC types
    let client_methods: Vec<_> = method_info
        .iter()
        .map(|(method_name, request_type, response_type, _path, _, is_ss, is_cs, _, _)| {
            // Reference the procedure constant instead of hardcoding the path
            let const_name = format_ident!("{}", method_name.to_string().to_uppercase());
            let procedure_path = quote! { #procedures_mod_name::#const_name };

            match (*is_ss, *is_cs) {
                (false, false) => {
                    // Unary RPC
                    quote! {
                        /// Make a unary RPC call to this method.
                        ///
                        /// Returns `ConnectResponse<T>` which includes response metadata.
                        pub async fn #method_name(
                            &self,
                            request: &#request_type,
                        ) -> Result<connectrpc_axum_client::ConnectResponse<#response_type>, connectrpc_axum_client::ClientError> {
                            self.inner.call_unary(#procedure_path, request).await
                        }
                    }
                }
                (true, false) => {
                    // Server streaming RPC
                    quote! {
                        /// Make a server streaming RPC call to this method.
                        ///
                        /// The server sends multiple messages in response to a single request.
                        /// Returns a stream of response messages wrapped in `ConnectResponse`.
                        /// After the stream is consumed, trailers are available via `stream.trailers()`.
                        pub async fn #method_name(
                            &self,
                            request: &#request_type,
                        ) -> Result<
                            connectrpc_axum_client::ConnectResponse<
                                connectrpc_axum_client::Streaming<
                                    connectrpc_axum_client::FrameDecoder<
                                        impl ::futures::Stream<Item = Result<connectrpc_axum_client::Bytes, connectrpc_axum_client::ClientError>> + Unpin,
                                        #response_type
                                    >
                                >
                            >,
                            connectrpc_axum_client::ClientError
                        > {
                            self.inner.call_server_stream(#procedure_path, request).await
                        }
                    }
                }
                (false, true) => {
                    // Client streaming RPC
                    quote! {
                        /// Make a client streaming RPC call to this method.
                        ///
                        /// The client sends multiple messages and receives a single response.
                        ///
                        /// # Arguments
                        ///
                        /// * `request` - A stream of request messages
                        ///
                        /// # Returns
                        ///
                        /// Returns a single response wrapped in `ConnectResponse`.
                        pub async fn #method_name<S>(
                            &self,
                            request: S,
                        ) -> Result<connectrpc_axum_client::ConnectResponse<#response_type>, connectrpc_axum_client::ClientError>
                        where
                            S: ::futures::Stream<Item = #request_type> + Send + Unpin + 'static,
                        {
                            self.inner.call_client_stream(#procedure_path, request).await
                        }
                    }
                }
                (true, true) => {
                    // Bidirectional streaming RPC
                    quote! {
                        /// Make a bidirectional streaming RPC call to this method.
                        ///
                        /// Both client and server send streams of messages.
                        /// This requires HTTP/2 for full duplex operation.
                        ///
                        /// # Arguments
                        ///
                        /// * `request` - A stream of request messages
                        ///
                        /// # Returns
                        ///
                        /// Returns a stream of response messages wrapped in `ConnectResponse`.
                        /// After the stream is consumed, trailers are available via `stream.trailers()`.
                        pub async fn #method_name<S>(
                            &self,
                            request: S,
                        ) -> Result<
                            connectrpc_axum_client::ConnectResponse<
                                connectrpc_axum_client::Streaming<
                                    connectrpc_axum_client::FrameDecoder<
                                        impl ::futures::Stream<Item = Result<connectrpc_axum_client::Bytes, connectrpc_axum_client::ClientError>> + Unpin,
                                        #response_type
                                    >
                                >
                            >,
                            connectrpc_axum_client::ClientError
                        >
                        where
                            S: ::futures::Stream<Item = #request_type> + Send + Unpin + 'static,
                        {
                            self.inner.call_bidi_stream(#procedure_path, request).await
                        }
                    }
                }
            }
        })
        .collect();

    quote! {
        /// Service name constant.
        pub const #service_name_const: &str = #full_service_name;

        /// Procedure path constants for the service.
        pub mod #procedures_mod_name {
            #(#procedure_constants)*
        }

        /// Generated typed client for the Connect RPC service.
        ///
        /// This client provides typed methods for each RPC, wrapping the underlying
        /// [`ConnectClient`](connectrpc_axum_client::ConnectClient).
        ///
        /// # Example
        ///
        /// ```ignore
        /// let client = #client_name::new("http://localhost:3000")?;
        /// let response = client.say_hello(&request).await?;
        /// println!("Response: {:?}", response.into_inner());
        /// ```
        #[derive(Debug, Clone)]
        pub struct #client_name {
            inner: connectrpc_axum_client::ConnectClient,
        }

        impl #client_name {
            /// Create a new client with default settings.
            ///
            /// Uses JSON encoding by default. For protobuf or other options,
            /// use [`builder()`](Self::builder) instead.
            pub fn new<S: Into<String>>(base_url: S) -> Result<Self, connectrpc_axum_client::ClientBuildError> {
                Self::builder(base_url).build()
            }

            /// Create a new client builder with the given base URL.
            pub fn builder<S: Into<String>>(base_url: S) -> #client_builder_name {
                #client_builder_name {
                    inner: connectrpc_axum_client::ConnectClient::builder(base_url),
                }
            }

            /// Get the underlying [`ConnectClient`](connectrpc_axum_client::ConnectClient).
            ///
            /// Useful for advanced use cases like making dynamic calls.
            pub fn inner(&self) -> &connectrpc_axum_client::ConnectClient {
                &self.inner
            }

            #(#client_methods)*
        }

        /// Builder for configuring a [`#client_name`].
        #[derive(Debug)]
        pub struct #client_builder_name {
            inner: connectrpc_axum_client::ClientBuilder,
        }

        impl #client_builder_name {
            /// Use protobuf encoding for requests and responses.
            pub fn use_proto(mut self) -> Self {
                self.inner = self.inner.use_proto();
                self
            }

            /// Use JSON encoding for requests and responses (default).
            pub fn use_json(mut self) -> Self {
                self.inner = self.inner.use_json();
                self
            }

            /// Use a pre-configured HTTP transport.
            pub fn with_transport(mut self, transport: connectrpc_axum_client::HyperTransport) -> Self {
                self.inner = self.inner.with_transport(transport);
                self
            }

            /// Configure compression for outgoing requests.
            pub fn compression(mut self, config: connectrpc_axum_client::CompressionConfig) -> Self {
                self.inner = self.inner.compression(config);
                self
            }

            /// Set the compression encoding for outgoing request bodies.
            pub fn request_encoding(mut self, encoding: connectrpc_axum_client::CompressionEncoding) -> Self {
                self.inner = self.inner.request_encoding(encoding);
                self
            }

            /// Set the accepted compression encoding for responses.
            pub fn accept_encoding(mut self, encoding: connectrpc_axum_client::CompressionEncoding) -> Self {
                self.inner = self.inner.accept_encoding(encoding);
                self
            }

            /// Set the default timeout for all requests.
            pub fn timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.inner = self.inner.timeout(timeout);
                self
            }

            /// Enable HTTP/2 prior knowledge (h2c) for plain HTTP URLs.
            ///
            /// Required for bidirectional streaming over `http://` URLs.
            /// For `https://` URLs, HTTP/2 is negotiated via ALPN automatically.
            pub fn http2_prior_knowledge(mut self) -> Self {
                self.inner = self.inner.http2_prior_knowledge();
                self
            }

            /// Build the client.
            pub fn build(self) -> Result<#client_name, connectrpc_axum_client::ClientBuildError> {
                Ok(#client_name {
                    inner: self.inner.build()?,
                })
            }
        }
    }
}
