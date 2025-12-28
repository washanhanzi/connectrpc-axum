use convert_case::{Case, Casing};
use prost_build::{Service, ServiceGenerator};
use quote::{format_ident, quote};

#[derive(Default)]
pub struct AxumConnectServiceGenerator {
    include_tonic: bool,
}

impl AxumConnectServiceGenerator {
    pub fn with_tonic(include_tonic: bool) -> Self {
        Self { include_tonic }
    }
}

impl ServiceGenerator for AxumConnectServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        let service_module_name = format_ident!("{}", service.name.to_lowercase());

        // Remove "Service" suffix from service name to avoid duplication (e.g., HelloWorldService -> HelloWorld)
        let service_base_name = service
            .name
            .strip_suffix("Service")
            .unwrap_or(&service.name);

        let service_builder_name = format_ident!("{}ServiceBuilder", service_base_name);

        // Extract request and response types for each method
        let method_info: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                let method_name = format_ident!("{}", method.name.to_case(Case::Snake));
                let request_type: proc_macro2::TokenStream =
                    method.input_type.parse().expect("invalid request type path");
                let response_type: proc_macro2::TokenStream =
                    method.output_type.parse().expect("invalid response type path");
                let path = format!(
                    "/{}.{}/{}",
                    service.package, service.proto_name, method.proto_name
                );
                let stream_assoc = format_ident!("{}Stream", method.proto_name);
                let is_server_streaming = method.server_streaming;
                let is_client_streaming = method.client_streaming;
                (
                    method_name,
                    request_type,
                    response_type,
                    path,
                    stream_assoc,
                    is_server_streaming,
                    is_client_streaming,
                )
            })
            .collect();

        // Generate Connect-only builder methods
        // Skip client streaming and bidirectional streaming methods (not supported by Connect)
        let connect_builder_methods: Vec<_> = method_info
            .iter()
            .filter(|(_method_name, _request_type, _response_type, _path, _assoc, _ss, is_cs)| {
                // Only include methods that are NOT client streaming
                // (unary and server-streaming are supported by Connect)
                !is_cs
            })
            .map(|(method_name, _request_type, _response_type, path, _assoc, is_ss, _is_cs)| {
                // Different signatures for streaming vs unary
                if *is_ss {
                    // Server streaming - use ConnectStreamHandlerWrapper
                    quote! {
                        /// Register a handler for this RPC method (server streaming)
                        pub fn #method_name<F, T>(self, handler: F) -> #service_builder_name<S>
                        where
                            connectrpc_axum::handler::ConnectStreamHandlerWrapper<F>: axum::handler::Handler<T, S>,
                            F: Clone + Send + Sync + 'static,
                            T: 'static,
                        {
                            let method_router = connectrpc_axum::handler::post_connect_stream(handler);
                            #service_builder_name {
                                router: self.router.route(#path, method_router),
                            }
                        }
                    }
                } else {
                    // Unary - use ConnectHandlerWrapper
                    quote! {
                        /// Register a handler for this RPC method (unary)
                        pub fn #method_name<F, T>(self, handler: F) -> #service_builder_name<S>
                        where
                            connectrpc_axum::handler::ConnectHandlerWrapper<F>: axum::handler::Handler<T, S>,
                            F: Clone + Send + Sync + 'static,
                            T: 'static,
                        {
                            let method_router = connectrpc_axum::handler::post_connect(handler);
                            #service_builder_name {
                                router: self.router.route(#path, method_router),
                            }
                        }
                    }
                }
            })
            .collect();

        // Check if service has any client streaming or bidirectional streaming methods
        let has_client_streaming = method_info.iter().any(|(_, _, _, _, _, _, is_cs)| *is_cs);

        // Only generate Tonic-compatible code if tonic is enabled and no client streaming
        let (tonic_module_bits, tonic_out_of_module) =
            if self.include_tonic && !has_client_streaming {
                // Tonic-related identifiers (only needed when tonic is enabled)
                let tonic_builder_name =
                    format_ident!("{}ServiceTonicCompatibleBuilder", service_base_name);
                let tonic_server_builder_name =
                    format_ident!("{}ServiceTonicCompatibleServerBuilder", service_base_name);
                let tonic_service_name = format_ident!("{}TonicService", service_base_name);

                // Tonic server trait paths (e.g., hello_world_service_server::HelloWorldService)
                let server_mod_name =
                    format_ident!("{}_server", service.proto_name.to_case(Case::Snake));
                let tonic_trait_ident = format_ident!("{}", service.proto_name);
                let tonic_server_type_name = format_ident!("{}Server", service.proto_name);

                // Generate field names for tonic builder field assignments
                let field_names: Vec<_> = method_info
                    .iter()
                    .map(|(name, _, _, _, _, _, _)| name)
                    .collect();

                // Generate Tonic-compatible builder methods
                let tonic_builder_methods: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, path, _assoc, is_ss, _is_cs)| {
                            let field_assignments: Vec<_> = field_names
                                .iter()
                                .map(|field_name| {
                                    quote! { #field_name: self.#field_name }
                                })
                                .collect();

                            if *is_ss {
                                // Server streaming - use TonicCompatibleStreamHandlerWrapper
                                quote! {
                                    /// Register a handler for this RPC method (server streaming, only Tonic-compatible extractors/responses allowed)
                                    pub fn #method_name<F, T>(mut self, handler: F) -> #tonic_builder_name<S>
                                    where
                                        connectrpc_axum::handler::TonicCompatibleStreamHandlerWrapper<F>:
                                            axum::handler::Handler<T, S>
                                            + connectrpc_axum::handler::IntoStreamFactory<T, #request_type, #response_type, S>,
                                        F: Clone + Send + Sync + 'static,
                                        T: 'static,
                                    {
                                        // Add route to router progressively
                                        let method_router = connectrpc_axum::handler::post_connect_tonic_stream(handler.clone());

                                        // Store factory (needs &S later to materialize the boxed stream call)
                                        let wrapper = connectrpc_axum::handler::TonicCompatibleStreamHandlerWrapper(handler);
                                        let factory = <connectrpc_axum::handler::TonicCompatibleStreamHandlerWrapper<F> as
                                            connectrpc_axum::handler::IntoStreamFactory<
                                                T, #request_type, #response_type, S
                                            >>::into_stream_factory(wrapper);
                                        self.#method_name = Some(factory);
                                        self.router = self.router.route(#path, method_router);

                                        #tonic_builder_name {
                                            #(#field_assignments,)*
                                            router: self.router,
                                        }
                                    }
                                }
                            } else {
                                // Unary - use TonicCompatibleHandlerWrapper
                                quote! {
                                    /// Register a handler for this RPC method (unary, only Tonic-compatible extractors/responses allowed)
                                    pub fn #method_name<F, T>(mut self, handler: F) -> #tonic_builder_name<S>
                                    where
                                        connectrpc_axum::handler::TonicCompatibleHandlerWrapper<F>:
                                            axum::handler::Handler<T, S>
                                            + connectrpc_axum::handler::IntoFactory<T, #request_type, #response_type, S>,
                                        F: Clone + Send + Sync + 'static,
                                        T: 'static,
                                    {
                                        // Add route to router progressively
                                        let method_router = connectrpc_axum::handler::post_connect_tonic(handler.clone());

                                        // Store factory (needs &S later to materialize the boxed call)
                                        let wrapper = connectrpc_axum::handler::TonicCompatibleHandlerWrapper(handler);
                                        let factory = <connectrpc_axum::handler::TonicCompatibleHandlerWrapper<F> as
                                            connectrpc_axum::handler::IntoFactory<
                                                T, #request_type, #response_type, S
                                            >>::into_factory(wrapper);
                                        self.#method_name = Some(factory);
                                        self.router = self.router.route(#path, method_router);

                                        #tonic_builder_name {
                                            #(#field_assignments,)*
                                            router: self.router,
                                        }
                                    }
                                }
                            }
                        },
                    )
                    .collect();

                // Generate tonic service handler fields
                let tonic_handler_fields: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, _is_cs)| {
                            if *is_ss {
                                quote! {
                                    pub #method_name: Option<
                                        Box<dyn Fn(&S) -> BoxedStreamCall<#request_type, #response_type> + Send + Sync>
                                    >
                                }
                            } else {
                                quote! {
                                    pub #method_name: Option<
                                        Box<dyn Fn(&S) -> BoxedCall<#request_type, #response_type> + Send + Sync>
                                    >
                                }
                            }
                        },
                    )
                    .collect();

                // Generate tonic server handler fields
                let tonic_server_handler_fields: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, _is_cs)| {
                            if *is_ss {
                                quote! {
                                    pub #method_name: Option<BoxedStreamCall<#request_type, #response_type>>
                                }
                            } else {
                                quote! {
                                    pub #method_name: Option<BoxedCall<#request_type, #response_type>>
                                }
                            }
                        },
                    )
                    .collect();

                // Generate final tonic service handler fields
                let tonic_service_handler_fields: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, _is_cs)| {
                            if *is_ss {
                                quote! {
                                    #method_name: connectrpc_axum::handler::BoxedStreamCall<#request_type, #response_type>
                                }
                            } else {
                                quote! {
                                    #method_name: connectrpc_axum::handler::BoxedCall<#request_type, #response_type>
                                }
                            }
                        },
                    )
                    .collect();

                // Generate field initializers for tonic builders
                let tonic_field_init: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, _request_type, _response_type, _path, _assoc, _ss, _is_cs)| {
                            quote! { #method_name: None }
                        },
                    )
                    .collect();

                // Generate handlers for build() with unimplemented fallbacks - no state version
                let tonic_build_handlers_no_state: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, _is_cs)| {
                            if *is_ss {
                                quote! {
                                    let #method_name: BoxedStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .map(|mk| mk(&()))
                                            .unwrap_or_else(|| unimplemented_boxed_stream_call());
                                }
                            } else {
                                quote! {
                                    let #method_name: BoxedCall<#request_type, #response_type> =
                                        self.#method_name
                                            .map(|mk| mk(&()))
                                            .unwrap_or_else(|| unimplemented_boxed_call());
                                }
                            }
                        },
                    )
                    .collect();

                // Generate handlers for build() with unimplemented fallbacks - with state version
                let tonic_build_handlers_with_state: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, _is_cs)| {
                            if *is_ss {
                                quote! {
                                    let #method_name: BoxedStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .unwrap_or_else(|| unimplemented_boxed_stream_call());
                                }
                            } else {
                                quote! {
                                    let #method_name: BoxedCall<#request_type, #response_type> =
                                        self.#method_name
                                            .unwrap_or_else(|| unimplemented_boxed_call());
                                }
                            }
                        },
                    )
                    .collect();

                // Generate field names for with_state mapping
                let with_state_field_mapping: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, _request_type, _response_type, _path, _assoc, _ss, _is_cs)| {
                            quote! { #method_name: self.#method_name.map(|mk| mk(&state)) }
                        },
                    )
                    .collect();

                // Generate field names for final service creation
                let service_field_names: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, _request_type, _response_type, _path, _assoc, _ss, _is_cs)| {
                            quote! { #method_name }
                        },
                    )
                    .collect();

                // Generate tonic trait associated types for server-streaming methods
                let tonic_assoc_types: Vec<_> = method_info
                    .iter()
                    .filter_map(|(_method_name, _req, resp, _path, assoc, is_ss, _is_cs)| {
                        if *is_ss {
                            Some(quote! {
                                type #assoc = std::pin::Pin<Box<dyn ::futures::Stream<Item = Result<#resp, ::tonic::Status>> + Send + 'static>>;
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                // Generate tonic trait method impls (unary and server-streaming both call the same boxed handler)
                let tonic_trait_methods: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, assoc, is_ss, _is_cs)| {
                            if *is_ss {
                                quote! {
                                    async fn #method_name(
                                        &self,
                                        request: ::tonic::Request<#request_type>,
                                    ) -> Result<::tonic::Response<Self::#assoc>, ::tonic::Status> {
                                        let req = connectrpc_axum::request::ConnectRequest(request.into_inner());
                                        match (self.#method_name)(req).await {
                                            Ok(response) => {
                                                // Extract the stream from StreamBody
                                                let stream = response.into_inner().into_inner();
                                                // Map ConnectError to tonic::Status and box the stream
                                                let mapped_stream = ::futures::StreamExt::map(
                                                    stream,
                                                    |result| result.map_err(|e| e.into())
                                                );
                                                let boxed_stream: Self::#assoc = Box::pin(mapped_stream);
                                                Ok(::tonic::Response::new(boxed_stream))
                                            }
                                            Err(err) => Err(err.into()),
                                        }
                                    }
                                }
                            } else {
                                quote! {
                                    async fn #method_name(
                                        &self,
                                        request: ::tonic::Request<#request_type>,
                                    ) -> Result<::tonic::Response<#response_type>, ::tonic::Status> {
                                        let req = connectrpc_axum::request::ConnectRequest(request.into_inner());
                                        match (self.#method_name)(req).await {
                                            Ok(response) => Ok(::tonic::Response::new(response.into_inner())),
                                            Err(err) => Err(err.into()),
                                        }
                                    }
                                }
                            }
                        },
                    )
                    .collect();

                let tonic_builder_structs = quote! {
                    /// TonicCompatibleBuilder has individual handler factories and progressive router
                    pub struct #tonic_builder_name<S = ()> {
                        #(#tonic_handler_fields,)*
                        pub router: axum::Router<S>,
                    }

                    /// Server-side builder with concrete handlers (state captured)
                    pub struct #tonic_server_builder_name<S = ()> {
                        #(#tonic_server_handler_fields,)*
                        pub router: axum::Router<S>,
                    }

                    impl<S> #tonic_builder_name<S>
                    where
                        S: Clone + Send + Sync + 'static,
                    {
                        pub fn new() -> Self {
                            Self {
                                #(#tonic_field_init,)*
                                router: axum::Router::new(),
                            }
                        }

                        #(#tonic_builder_methods)*

                        /// Apply state to router and handlers, returning server builder with concrete handlers
                        pub fn with_state<S2>(self, state: S) -> #tonic_server_builder_name<S2> {
                            let router = self.router.with_state(state.clone());
                            #tonic_server_builder_name {
                                #(#with_state_field_mapping,)*
                                router,
                            }
                        }
                    }

                    impl #tonic_builder_name<()> {
                        /// Build without state by converting factories with `()`
                        ///
                        /// Returns the router and gRPC server. Use [`MakeServiceBuilder`] to
                        /// apply [`ConnectLayer`] and combine with other services.
                        ///
                        /// [`MakeServiceBuilder`]: connectrpc_axum::MakeServiceBuilder
                        /// [`ConnectLayer`]: connectrpc_axum::ConnectLayer
                        pub fn build(self) -> (axum::Router, #server_mod_name::#tonic_server_type_name<#tonic_service_name>) {
                            let router = self.router;
                            #(#tonic_build_handlers_no_state)*

                            let tonic_service = #tonic_service_name {
                                #(#service_field_names,)*
                            };

                            let grpc_server = #server_mod_name::#tonic_server_type_name::new(tonic_service);
                            (router, grpc_server)
                        }
                    }

                    impl #tonic_server_builder_name {
                        /// Build with state already captured in handlers
                        ///
                        /// Returns the router and gRPC server. Use [`MakeServiceBuilder`] to
                        /// apply [`ConnectLayer`] and combine with other services.
                        ///
                        /// [`MakeServiceBuilder`]: connectrpc_axum::MakeServiceBuilder
                        /// [`ConnectLayer`]: connectrpc_axum::ConnectLayer
                        pub fn build(self) -> (axum::Router, #server_mod_name::#tonic_server_type_name<#tonic_service_name>) {
                            let router = self.router;
                            #(#tonic_build_handlers_with_state)*

                            let tonic_service = #tonic_service_name {
                                #(#service_field_names,)*
                            };

                            let grpc_server = #server_mod_name::#tonic_server_type_name::new(tonic_service);
                            (router, grpc_server)
                        }
                    }
                };

                let module_bits = quote! {
                    // Local aliases to reduce fully-qualified verbosity in generated code
                    type BoxedCall<Req, Resp> = connectrpc_axum::handler::BoxedCall<Req, Resp>;
                    type BoxedStreamCall<Req, Resp> = connectrpc_axum::handler::BoxedStreamCall<Req, Resp>;

                    fn unimplemented_boxed_call<Req, Resp>() -> BoxedCall<Req, Resp>
                    where
                        Req: Send + Sync + 'static,
                        Resp: Send + Sync + 'static,
                    {
                        connectrpc_axum::handler::unimplemented_boxed_call::<Req, Resp>()
                    }

                    fn unimplemented_boxed_stream_call<Req, Resp>() -> BoxedStreamCall<Req, Resp>
                    where
                        Req: Send + Sync + 'static,
                        Resp: Send + Sync + 'static,
                    {
                        connectrpc_axum::handler::unimplemented_boxed_stream_call::<Req, Resp>()
                    }

                    #tonic_builder_structs
                };

                let out_of_module = quote! {
                    /// Generated Tonic-compatible service that holds boxed calls.
                    /// This struct directly implements the Tonic trait, following Tonic's idiomatic
                    /// approach where the trait serves as the primary interface.
                    pub struct #tonic_service_name {
                        #(#tonic_service_handler_fields,)*
                    }

                    // Implement the tonic service trait for the generated boxed service.
                    // The trait implementation directly calls the boxed handlers, avoiding
                    // unnecessary intermediate wrapper methods.
                    #[::tonic::async_trait]
                    impl #server_mod_name::#tonic_trait_ident for #tonic_service_name {
                        #(#tonic_assoc_types)*

                        #(#tonic_trait_methods)*
                    }
                };

                (module_bits, out_of_module)
            } else {
                (quote! {}, quote! {})
            };

        let routes_fn = quote! {
            pub mod #service_module_name {
                #[allow(unused_imports)]
                use super::*;

                /// Connect-only service builder (flexible extractors)
                pub struct #service_builder_name<S = ()> {
                    pub router: axum::Router<S>,
                }

                impl<S> #service_builder_name<S>
                where
                    S: Clone + Send + Sync + 'static,
                {
                    pub fn new() -> Self {
                        Self {
                            router: axum::Router::new(),
                        }
                    }

                    /// Apply state to router, transforming to builder with new state
                    pub fn with_state<S2>(self, state: S) -> #service_builder_name<S2> {
                        #service_builder_name {
                            router: self.router.with_state(state),
                        }
                    }

                    #(#connect_builder_methods)*
                }

                impl #service_builder_name<()> {
                    /// Build the final Connect RPC router with all registered handlers.
                    ///
                    /// Use [`MakeServiceBuilder`] to apply [`ConnectLayer`] and combine
                    /// with other services.
                    ///
                    /// [`MakeServiceBuilder`]: connectrpc_axum::MakeServiceBuilder
                    /// [`ConnectLayer`]: connectrpc_axum::ConnectLayer
                    pub fn build(self) -> axum::Router<()> {
                        self.router
                    }
                }

                #tonic_module_bits
            }

            #tonic_out_of_module
        };

        buf.push_str(&routes_fn.to_string());
    }
}
