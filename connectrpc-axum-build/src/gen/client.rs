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

/// RPC type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RpcType {
    Unary,
    ServerStream,
    ClientStream,
    BidiStream,
}

impl RpcType {
    fn from_streaming(is_server_streaming: bool, is_client_streaming: bool) -> Self {
        match (is_server_streaming, is_client_streaming) {
            (false, false) => RpcType::Unary,
            (true, false) => RpcType::ServerStream,
            (false, true) => RpcType::ClientStream,
            (true, true) => RpcType::BidiStream,
        }
    }
}

/// Generate the Connect RPC client code.
pub fn generate_connect_client(
    service: &Service,
    method_info: &[MethodInfo],
) -> proc_macro2::TokenStream {
    // Client module name (e.g., hello_world_service_connect_client)
    let client_module_name = format_ident!(
        "{}_connect_client",
        service.name.to_case(Case::Snake)
    );

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

    // Generate interceptor fields (shared between client and builder structs)
    let interceptor_fields: Vec<_> = method_info
        .iter()
        .map(|(method_name, request_type, response_type, _, _, is_ss, is_cs, _, _)| {
            let field_name = format_ident!("{}_interceptors", method_name);
            let rpc_type = RpcType::from_streaming(*is_ss, *is_cs);

            match rpc_type {
                RpcType::Unary => quote! {
                    #field_name: connectrpc_axum_client::UnaryInterceptors<#request_type, #response_type>
                },
                RpcType::ServerStream => quote! {
                    #field_name: connectrpc_axum_client::ServerStreamInterceptors<#request_type, #response_type>
                },
                RpcType::ClientStream => quote! {
                    #field_name: connectrpc_axum_client::ClientStreamInterceptors<#request_type, #response_type>
                },
                RpcType::BidiStream => quote! {
                    #field_name: connectrpc_axum_client::BidiStreamInterceptors<#request_type, #response_type>
                },
            }
        })
        .collect();

    // Generate builder interceptor field initializers (all default)
    let builder_interceptor_defaults: Vec<_> = method_info
        .iter()
        .map(|(method_name, _, _, _, _, _, _, _, _)| {
            let field_name = format_ident!("{}_interceptors", method_name);
            quote! { #field_name: Default::default() }
        })
        .collect();

    // Generate builder methods for setting interceptors
    let builder_interceptor_methods: Vec<_> = method_info
        .iter()
        .flat_map(|(method_name, request_type, response_type, _, _, is_ss, is_cs, _, _)| {
            let field_name = format_ident!("{}_interceptors", method_name);
            let rpc_type = RpcType::from_streaming(*is_ss, *is_cs);

            let mut methods = Vec::new();

            match rpc_type {
                RpcType::Unary => {
                    // with_before_{method}
                    let before_method = format_ident!("with_before_{}", method_name);
                    methods.push(quote! {
                        /// Set a "before" interceptor for this method.
                        ///
                        /// The interceptor is called before the request is sent, allowing
                        /// modification of headers and the request body.
                        pub fn #before_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: connectrpc_axum_client::TypedMutInterceptor<#request_type>,
                        {
                            self.#field_name.before = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });

                    // with_after_{method}
                    let after_method = format_ident!("with_after_{}", method_name);
                    methods.push(quote! {
                        /// Set an "after" interceptor for this method.
                        ///
                        /// The interceptor is called after the response is received, allowing
                        /// inspection or modification of the response body.
                        pub fn #after_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: for<'a> connectrpc_axum_client::TypedInterceptor<connectrpc_axum_client::ResponseContext<'a>, #response_type>,
                        {
                            self.#field_name.after = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });
                }
                RpcType::ServerStream => {
                    // with_before_{method}
                    let before_method = format_ident!("with_before_{}", method_name);
                    methods.push(quote! {
                        /// Set a "before" interceptor for this method.
                        ///
                        /// The interceptor is called before the request is sent, allowing
                        /// modification of headers and the request body.
                        pub fn #before_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: connectrpc_axum_client::TypedMutInterceptor<#request_type>,
                        {
                            self.#field_name.before = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });

                    // with_on_receive_{method}
                    let on_receive_method = format_ident!("with_on_receive_{}", method_name);
                    methods.push(quote! {
                        /// Set an "on_receive" interceptor for this method.
                        ///
                        /// The interceptor is called for each message received from the server stream.
                        pub fn #on_receive_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: for<'a> connectrpc_axum_client::TypedInterceptor<connectrpc_axum_client::StreamContext<'a>, #response_type>,
                        {
                            self.#field_name.on_receive = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });
                }
                RpcType::ClientStream => {
                    // with_on_send_{method}
                    let on_send_method = format_ident!("with_on_send_{}", method_name);
                    methods.push(quote! {
                        /// Set an "on_send" interceptor for this method.
                        ///
                        /// The interceptor is called for each message before it is sent to the server.
                        pub fn #on_send_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: for<'a> connectrpc_axum_client::TypedInterceptor<connectrpc_axum_client::StreamContext<'a>, #request_type>,
                        {
                            self.#field_name.on_send = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });

                    // with_after_{method}
                    let after_method = format_ident!("with_after_{}", method_name);
                    methods.push(quote! {
                        /// Set an "after" interceptor for this method.
                        ///
                        /// The interceptor is called after the final response is received.
                        pub fn #after_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: for<'a> connectrpc_axum_client::TypedInterceptor<connectrpc_axum_client::ResponseContext<'a>, #response_type>,
                        {
                            self.#field_name.after = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });
                }
                RpcType::BidiStream => {
                    // with_on_send_{method}
                    let on_send_method = format_ident!("with_on_send_{}", method_name);
                    methods.push(quote! {
                        /// Set an "on_send" interceptor for this method.
                        ///
                        /// The interceptor is called for each message before it is sent to the server.
                        pub fn #on_send_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: for<'a> connectrpc_axum_client::TypedInterceptor<connectrpc_axum_client::StreamContext<'a>, #request_type>,
                        {
                            self.#field_name.on_send = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });

                    // with_on_receive_{method}
                    let on_receive_method = format_ident!("with_on_receive_{}", method_name);
                    methods.push(quote! {
                        /// Set an "on_receive" interceptor for this method.
                        ///
                        /// The interceptor is called for each message received from the server stream.
                        pub fn #on_receive_method<I>(mut self, interceptor: I) -> Self
                        where
                            I: for<'a> connectrpc_axum_client::TypedInterceptor<connectrpc_axum_client::StreamContext<'a>, #response_type>,
                        {
                            self.#field_name.on_receive = Some(::std::sync::Arc::new(interceptor));
                            self
                        }
                    });
                }
            }

            methods
        })
        .collect();

    // Generate interceptor field assignments in build()
    let build_interceptor_assignments: Vec<_> = method_info
        .iter()
        .map(|(method_name, _, _, _, _, _, _, _, _)| {
            let field_name = format_ident!("{}_interceptors", method_name);
            quote! { #field_name: self.#field_name }
        })
        .collect();

    // Generate interceptor field clone assignments for Clone impl
    let clone_interceptor_assignments: Vec<_> = method_info
        .iter()
        .map(|(method_name, _, _, _, _, _, _, _, _)| {
            let field_name = format_ident!("{}_interceptors", method_name);
            quote! { #field_name: self.#field_name.clone() }
        })
        .collect();

    // Generate typed client methods for all RPC types
    let client_methods: Vec<_> = method_info
        .iter()
        .map(|(method_name, request_type, response_type, _path, _, is_ss, is_cs, _, _)| {
            // Reference the procedure constant instead of hardcoding the path
            let const_name = format_ident!("{}", method_name.to_string().to_uppercase());
            let procedure_path = quote! { super::#procedures_mod_name::#const_name };
            let interceptors_field = format_ident!("{}_interceptors", method_name);
            let rpc_type = RpcType::from_streaming(*is_ss, *is_cs);

            match rpc_type {
                RpcType::Unary => {
                    quote! {
                        /// Make a unary RPC call to this method.
                        ///
                        /// Returns `ConnectResponse<T>` which includes response metadata.
                        pub async fn #method_name(
                            &self,
                            request: &#request_type,
                        ) -> Result<connectrpc_axum_client::ConnectResponse<#response_type>, connectrpc_axum_client::ClientError> {
                            let mut request = request.clone();

                            // Before interceptor - may modify headers and request body
                            let mut interceptor_headers = connectrpc_axum_client::HeaderMap::new();
                            if let Some(ref interceptor) = self.#interceptors_field.before {
                                let mut ctx = connectrpc_axum_client::RequestContext::new(#procedure_path, &mut interceptor_headers);
                                interceptor.intercept(&mut ctx, &mut request)?;
                            }

                            let options = connectrpc_axum_client::CallOptions::new().headers(interceptor_headers);
                            let mut response: connectrpc_axum_client::ConnectResponse<#response_type> =
                                self.inner.call_unary_with_options(#procedure_path, &request, options).await?;

                            // After interceptor
                            if let Some(ref interceptor) = self.#interceptors_field.after {
                                let response_headers = response.metadata().headers().clone();
                                let ctx = connectrpc_axum_client::ResponseContext::new(
                                    #procedure_path,
                                    &response_headers,
                                );
                                interceptor.intercept(&ctx, response.get_mut())?;
                            }

                            Ok(response)
                        }
                    }
                }
                RpcType::ServerStream => {
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
                                connectrpc_axum_client::TypedReceiveStreaming<
                                    connectrpc_axum_client::FrameDecoder<
                                        impl ::futures::Stream<Item = Result<connectrpc_axum_client::Bytes, connectrpc_axum_client::ClientError>> + Unpin + use<'_>,
                                        #response_type
                                    >,
                                    #response_type
                                >
                            >,
                            connectrpc_axum_client::ClientError
                        > {
                            let mut request = request.clone();

                            // Before interceptor - may modify headers and request body
                            let mut interceptor_headers = connectrpc_axum_client::HeaderMap::new();
                            if let Some(ref interceptor) = self.#interceptors_field.before {
                                let mut ctx = connectrpc_axum_client::RequestContext::new(#procedure_path, &mut interceptor_headers);
                                interceptor.intercept(&mut ctx, &mut request)?;
                            }

                            let options = connectrpc_axum_client::CallOptions::new().headers(interceptor_headers.clone());
                            let response = self.inner.call_server_stream_with_options(#procedure_path, &request, options).await?;

                            // Get headers for context
                            let response_headers = response.metadata().headers().clone();

                            // Wrap the stream with typed interceptor
                            let on_receive = self.#interceptors_field.on_receive.clone();
                            Ok(response.map(|streaming| {
                                connectrpc_axum_client::TypedReceiveStreaming::new(
                                    streaming.get_inner(),
                                    on_receive,
                                    #procedure_path.to_string(),
                                    connectrpc_axum_client::StreamType::ServerStream,
                                    interceptor_headers,
                                    response_headers,
                                )
                            }))
                        }
                    }
                }
                RpcType::ClientStream => {
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
                        ///
                        /// # Error Handling
                        ///
                        /// If an `on_send` interceptor returns an error, the stream is
                        /// aborted and the error is returned immediately.
                        pub async fn #method_name<S>(
                            &self,
                            request: S,
                        ) -> Result<connectrpc_axum_client::ConnectResponse<#response_type>, connectrpc_axum_client::ClientError>
                        where
                            S: ::futures::Stream<Item = #request_type> + Send + Unpin + 'static,
                        {
                            use ::futures::StreamExt;

                            let on_send = self.#interceptors_field.on_send.clone();
                            let procedure = #procedure_path.to_string();
                            let interceptor_error: ::std::sync::Arc<::std::sync::Mutex<Option<connectrpc_axum_client::ClientError>>> =
                                ::std::sync::Arc::new(::std::sync::Mutex::new(None));
                            let err_capture = interceptor_error.clone();
                            let request_headers = connectrpc_axum_client::HeaderMap::new();

                            // Use scan to apply interceptor; abort stream on first error
                            let wrapped = request.scan((), move |_state, mut msg| {
                                if let Some(ref i) = on_send {
                                    let ctx = connectrpc_axum_client::StreamContext::new(
                                        &procedure,
                                        connectrpc_axum_client::StreamType::ClientStream,
                                        &request_headers,
                                        None,
                                    );
                                    match i.intercept(&ctx, &mut msg) {
                                        Ok(()) => ::std::future::ready(Some(msg)),
                                        Err(e) => {
                                            // Store error and terminate stream
                                            *err_capture.lock().unwrap() = Some(e);
                                            ::std::future::ready(None)
                                        }
                                    }
                                } else {
                                    ::std::future::ready(Some(msg))
                                }
                            });

                            let options = connectrpc_axum_client::CallOptions::new();
                            let mut response: connectrpc_axum_client::ConnectResponse<#response_type> =
                                self.inner.call_client_stream_with_options(#procedure_path, wrapped, options).await?;

                            // Check if on_send interceptor aborted the stream
                            if let Some(e) = interceptor_error.lock().unwrap().take() {
                                return Err(e);
                            }

                            // After interceptor
                            if let Some(ref interceptor) = self.#interceptors_field.after {
                                let response_headers = response.metadata().headers().clone();
                                let ctx = connectrpc_axum_client::ResponseContext::new(
                                    #procedure_path,
                                    &response_headers,
                                );
                                interceptor.intercept(&ctx, response.get_mut())?;
                            }

                            Ok(response)
                        }
                    }
                }
                RpcType::BidiStream => {
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
                        ///
                        /// # Error Handling
                        ///
                        /// If an `on_send` interceptor returns an error, the send stream is
                        /// aborted (remaining messages are not sent). If an `on_receive`
                        /// interceptor returns an error, it is yielded as a stream error item.
                        pub async fn #method_name<S>(
                            &self,
                            request: S,
                        ) -> Result<
                            connectrpc_axum_client::ConnectResponse<
                                connectrpc_axum_client::TypedReceiveStreaming<
                                    connectrpc_axum_client::FrameDecoder<
                                        impl ::futures::Stream<Item = Result<connectrpc_axum_client::Bytes, connectrpc_axum_client::ClientError>> + Unpin + use<'_, S>,
                                        #response_type
                                    >,
                                    #response_type
                                >
                            >,
                            connectrpc_axum_client::ClientError
                        >
                        where
                            S: ::futures::Stream<Item = #request_type> + Send + Unpin + 'static,
                        {
                            use ::futures::StreamExt;

                            // Wrap the input stream with on_send interceptor
                            let on_send = self.#interceptors_field.on_send.clone();
                            let procedure = #procedure_path.to_string();
                            let request_headers = connectrpc_axum_client::HeaderMap::new();

                            // Use scan to apply interceptor; abort stream on first error
                            let wrapped = request.scan((), move |_state, mut msg| {
                                if let Some(ref i) = on_send {
                                    let ctx = connectrpc_axum_client::StreamContext::new(
                                        &procedure,
                                        connectrpc_axum_client::StreamType::BidiStream,
                                        &request_headers,
                                        None,
                                    );
                                    match i.intercept(&ctx, &mut msg) {
                                        Ok(()) => ::std::future::ready(Some(msg)),
                                        Err(_e) => ::std::future::ready(None), // Terminate send stream
                                    }
                                } else {
                                    ::std::future::ready(Some(msg))
                                }
                            });

                            let options = connectrpc_axum_client::CallOptions::new();
                            let response = self.inner.call_bidi_stream_with_options(#procedure_path, wrapped, options).await?;

                            // Get headers for context
                            let response_headers = response.metadata().headers().clone();
                            let req_headers = connectrpc_axum_client::HeaderMap::new();

                            // Wrap the response stream with typed interceptor
                            let on_receive = self.#interceptors_field.on_receive.clone();
                            Ok(response.map(|streaming| {
                                connectrpc_axum_client::TypedReceiveStreaming::new(
                                    streaming.get_inner(),
                                    on_receive,
                                    #procedure_path.to_string(),
                                    connectrpc_axum_client::StreamType::BidiStream,
                                    req_headers,
                                    response_headers,
                                )
                            }))
                        }
                    }
                }
            }
        })
        .collect();

    quote! {
        /// Procedure path constants for the service.
        #[allow(dead_code)]
        pub mod #procedures_mod_name {
            #(#procedure_constants)*
        }

        /// Generated Connect RPC client module.
        #[allow(dead_code)]
        pub mod #client_module_name {
            #[allow(unused_imports)]
            use super::*;

            /// Generated typed client for the Connect RPC service.
            ///
            /// This client provides typed methods for each RPC, wrapping the underlying
            /// [`ConnectClient`](connectrpc_axum_client::ConnectClient).
            ///
            /// # Example
            ///
            /// ```ignore
            /// // Simple usage (panics on error)
            /// let client = #client_name::new("http://localhost:3000");
            ///
            /// // With error handling
            /// let client = #client_name::builder("http://localhost:3000").build()?;
            ///
            /// let response = client.say_hello(&request).await?;
            /// println!("Response: {:?}", response.into_inner());
            /// ```
            #[derive(Debug)]
            pub struct #client_name {
                inner: connectrpc_axum_client::ConnectClient,
                #(#interceptor_fields,)*
            }

            impl Clone for #client_name {
                fn clone(&self) -> Self {
                    Self {
                        inner: self.inner.clone(),
                        // Interceptors are wrapped in Arc, so cloning is cheap
                        #(#clone_interceptor_assignments,)*
                    }
                }
            }

            impl #client_name {
                /// Create a new client with default settings.
                ///
                /// Uses JSON encoding by default.
                ///
                /// # Panics
                ///
                /// Panics if the client cannot be built (e.g., TLS initialization failure).
                /// Use [`builder()`](Self::builder) if you need to handle errors.
                pub fn new<S: Into<String>>(base_url: S) -> Self {
                    Self::builder(base_url).build().expect("failed to build client")
                }

                /// Create a new client builder with the given base URL.
                ///
                /// Use builder methods to configure the client, then call `build()` to create it.
                ///
                /// # Example
                ///
                /// ```ignore
                /// let client = #client_name::builder("http://localhost:3000")
                ///     .use_proto()
                ///     .timeout(std::time::Duration::from_secs(30))
                ///     .with_before_say(|ctx, req| {
                ///         // Validate request before sending
                ///         Ok(())
                ///     })
                ///     .build()?;
                /// ```
                pub fn builder<S: Into<String>>(base_url: S) -> #client_builder_name {
                    #client_builder_name {
                        inner: connectrpc_axum_client::ConnectClient::builder(base_url),
                        #(#builder_interceptor_defaults,)*
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
            pub struct #client_builder_name {
                inner: connectrpc_axum_client::ClientBuilder,
                #(#interceptor_fields,)*
            }

            impl ::std::fmt::Debug for #client_builder_name {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.debug_struct(stringify!(#client_builder_name))
                        .field("inner", &self.inner)
                        .finish_non_exhaustive()
                }
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

                #(#builder_interceptor_methods)*

                /// Build the client.
                pub fn build(self) -> Result<#client_name, connectrpc_axum_client::ClientBuildError> {
                    Ok(#client_name {
                        inner: self.inner.build()?,
                        #(#build_interceptor_assignments,)*
                    })
                }
            }
        }
    }
}
