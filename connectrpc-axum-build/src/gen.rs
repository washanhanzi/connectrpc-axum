use convert_case::{Case, Casing};
use prost_build::{Service, ServiceGenerator};
use prost_types::method_options::IdempotencyLevel;
use quote::{format_ident, quote};

/// Convert protobuf IdempotencyLevel to a token stream referencing connectrpc_axum's enum.
fn idempotency_level_tokens(level: Option<i32>) -> proc_macro2::TokenStream {
    match level.and_then(|v| IdempotencyLevel::try_from(v).ok()) {
        Some(IdempotencyLevel::NoSideEffects) => {
            quote! { connectrpc_axum::IdempotencyLevel::NoSideEffects }
        }
        Some(IdempotencyLevel::Idempotent) => {
            quote! { connectrpc_axum::IdempotencyLevel::Idempotent }
        }
        Some(IdempotencyLevel::IdempotencyUnknown) | None => {
            quote! { connectrpc_axum::IdempotencyLevel::Unknown }
        }
    }
}

/// Check if idempotency level is NoSideEffects (enables GET requests).
fn is_no_side_effects(level: Option<i32>) -> bool {
    matches!(
        level.and_then(|v| IdempotencyLevel::try_from(v).ok()),
        Some(IdempotencyLevel::NoSideEffects)
    )
}

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
                let request_type: proc_macro2::TokenStream = method
                    .input_type
                    .parse()
                    .expect("invalid request type path");
                let response_type: proc_macro2::TokenStream = method
                    .output_type
                    .parse()
                    .expect("invalid response type path");
                let path = format!(
                    "/{}.{}/{}",
                    service.package, service.proto_name, method.proto_name
                );
                let stream_assoc = format_ident!("{}Stream", method.proto_name);
                let is_server_streaming = method.server_streaming;
                let is_client_streaming = method.client_streaming;
                let idempotency_level = method.options.idempotency_level;
                let idempotency_tokens = idempotency_level_tokens(idempotency_level);
                (
                    method_name,
                    request_type,
                    response_type,
                    path,
                    stream_assoc,
                    is_server_streaming,
                    is_client_streaming,
                    idempotency_level,
                    idempotency_tokens,
                )
            })
            .collect();

        // Generate Connect-only builder methods for ALL streaming types
        // Uses the unified post_connect function which auto-detects RPC type from handler signature
        // For unary methods with NoSideEffects, automatically enables GET requests (per Connect spec)
        let connect_builder_methods: Vec<_> = method_info
            .iter()
            .map(|(method_name, _request_type, _response_type, path, _assoc, is_ss, is_cs, idempotency_level, idempotency_tokens)| {
                // Generate doc comment based on streaming type and idempotency
                let is_unary = !*is_ss && !*is_cs;
                let supports_get = is_unary && is_no_side_effects(*idempotency_level);

                let doc = match (*is_ss, *is_cs, supports_get) {
                    (false, false, true) => "Register a handler for this RPC method (unary, GET+POST enabled)",
                    (false, false, false) => "Register a handler for this RPC method (unary)",
                    (true, false, _) => "Register a handler for this RPC method (server streaming)",
                    (false, true, _) => "Register a handler for this RPC method (client streaming)",
                    (true, true, _) => "Register a handler for this RPC method (bidirectional streaming)",
                };

                // Generate constant name for idempotency level (e.g., GET_USER_IDEMPOTENCY)
                let idempotency_const_name = format_ident!(
                    "{}_IDEMPOTENCY",
                    method_name.to_string().to_uppercase()
                );

                // For unary methods with NoSideEffects, enable both GET and POST
                // This follows the Connect protocol spec where NO_SIDE_EFFECTS enables HTTP GET
                let method_router_expr = if supports_get {
                    quote! {
                        connectrpc_axum::handler::get_connect(handler.clone())
                            .merge(connectrpc_axum::handler::post_connect(handler))
                    }
                } else {
                    quote! {
                        connectrpc_axum::handler::post_connect(handler)
                    }
                };

                quote! {
                    /// Idempotency level for this RPC method.
                    #[allow(dead_code)]
                    pub const #idempotency_const_name: connectrpc_axum::IdempotencyLevel = #idempotency_tokens;

                    #[doc = #doc]
                    pub fn #method_name<F, T>(self, handler: F) -> #service_builder_name<S>
                    where
                        connectrpc_axum::handler::ConnectHandlerWrapper<F>: axum::handler::Handler<T, S>,
                        F: Clone + Send + Sync + 'static,
                        T: 'static,
                    {
                        let method_router = #method_router_expr;
                        #service_builder_name {
                            router: self.router.route(#path, method_router),
                        }
                    }
                }
            })
            .collect();

        // Generate Tonic-compatible code if tonic is enabled
        let (tonic_module_bits, tonic_out_of_module) = if self.include_tonic {
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
                .map(|(name, _, _, _, _, _, _, _, _)| name)
                .collect();

            // Generate Tonic-compatible builder methods
            let tonic_builder_methods: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, path, _assoc, is_ss, is_cs, _, _)| {
                            let field_assignments: Vec<_> = field_names
                                .iter()
                                .map(|field_name| {
                                    quote! { #field_name: self.#field_name }
                                })
                                .collect();

                            match (*is_ss, *is_cs) {
                                (false, false) => {
                                    // Unary - use TonicCompatibleHandlerWrapper
                                    quote! {
                                        /// Register a handler for this RPC method (unary)
                                        pub fn #method_name<F, T>(mut self, handler: F) -> #tonic_builder_name<S>
                                        where
                                            connectrpc_axum::tonic::TonicCompatibleHandlerWrapper<F>:
                                                axum::handler::Handler<T, S>
                                                + connectrpc_axum::tonic::IntoFactory<T, #request_type, #response_type, S>,
                                            F: Clone + Send + Sync + 'static,
                                            T: 'static,
                                        {
                                            // Add route to router progressively
                                            let method_router = connectrpc_axum::tonic::post_tonic_unary(handler.clone());

                                            // Store factory (needs &S later to materialize the boxed call)
                                            let wrapper = connectrpc_axum::tonic::TonicCompatibleHandlerWrapper(handler);
                                            let factory = <connectrpc_axum::tonic::TonicCompatibleHandlerWrapper<F> as
                                                connectrpc_axum::tonic::IntoFactory<
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
                                (true, false) => {
                                    // Server streaming - use TonicCompatibleStreamHandlerWrapper
                                    quote! {
                                        /// Register a handler for this RPC method (server streaming)
                                        pub fn #method_name<F, T>(mut self, handler: F) -> #tonic_builder_name<S>
                                        where
                                            connectrpc_axum::tonic::TonicCompatibleStreamHandlerWrapper<F>:
                                                axum::handler::Handler<T, S>
                                                + connectrpc_axum::tonic::IntoStreamFactory<T, #request_type, #response_type, S>,
                                            F: Clone + Send + Sync + 'static,
                                            T: 'static,
                                        {
                                            // Add route to router progressively
                                            let method_router = connectrpc_axum::tonic::post_tonic_stream(handler.clone());

                                            // Store factory (needs &S later to materialize the boxed stream call)
                                            let wrapper = connectrpc_axum::tonic::TonicCompatibleStreamHandlerWrapper(handler);
                                            let factory = <connectrpc_axum::tonic::TonicCompatibleStreamHandlerWrapper<F> as
                                                connectrpc_axum::tonic::IntoStreamFactory<
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
                                }
                                (false, true) => {
                                    // Client streaming - use TonicCompatibleClientStreamHandlerWrapper
                                    quote! {
                                        /// Register a handler for this RPC method (client streaming)
                                        pub fn #method_name<F, T>(mut self, handler: F) -> #tonic_builder_name<S>
                                        where
                                            connectrpc_axum::tonic::TonicCompatibleClientStreamHandlerWrapper<F>:
                                                axum::handler::Handler<T, S>
                                                + connectrpc_axum::tonic::IntoClientStreamFactory<T, #request_type, #response_type, S>,
                                            F: Clone + Send + Sync + 'static,
                                            T: 'static,
                                        {
                                            // Add route to router progressively
                                            let method_router = connectrpc_axum::tonic::post_tonic_client_stream(handler.clone());

                                            // Store factory (needs &S later to materialize the boxed client stream call)
                                            let wrapper = connectrpc_axum::tonic::TonicCompatibleClientStreamHandlerWrapper(handler);
                                            let factory = <connectrpc_axum::tonic::TonicCompatibleClientStreamHandlerWrapper<F> as
                                                connectrpc_axum::tonic::IntoClientStreamFactory<
                                                    T, #request_type, #response_type, S
                                                >>::into_client_stream_factory(wrapper);
                                            self.#method_name = Some(factory);
                                            self.router = self.router.route(#path, method_router);

                                            #tonic_builder_name {
                                                #(#field_assignments,)*
                                                router: self.router,
                                            }
                                        }
                                    }
                                }
                                (true, true) => {
                                    // Bidi streaming - use TonicCompatibleBidiStreamHandlerWrapper
                                    quote! {
                                        /// Register a handler for this RPC method (bidirectional streaming)
                                        pub fn #method_name<F, T>(mut self, handler: F) -> #tonic_builder_name<S>
                                        where
                                            connectrpc_axum::tonic::TonicCompatibleBidiStreamHandlerWrapper<F>:
                                                axum::handler::Handler<T, S>
                                                + connectrpc_axum::tonic::IntoBidiStreamFactory<T, #request_type, #response_type, S>,
                                            F: Clone + Send + Sync + 'static,
                                            T: 'static,
                                        {
                                            // Add route to router progressively
                                            let method_router = connectrpc_axum::tonic::post_tonic_bidi_stream(handler.clone());

                                            // Store factory (needs &S later to materialize the boxed bidi stream call)
                                            let wrapper = connectrpc_axum::tonic::TonicCompatibleBidiStreamHandlerWrapper(handler);
                                            let factory = <connectrpc_axum::tonic::TonicCompatibleBidiStreamHandlerWrapper<F> as
                                                connectrpc_axum::tonic::IntoBidiStreamFactory<
                                                    T, #request_type, #response_type, S
                                                >>::into_bidi_stream_factory(wrapper);
                                            self.#method_name = Some(factory);
                                            self.router = self.router.route(#path, method_router);

                                            #tonic_builder_name {
                                                #(#field_assignments,)*
                                                router: self.router,
                                            }
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
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, is_cs, _, _)| {
                            match (*is_ss, *is_cs) {
                                (false, false) => quote! {
                                    pub #method_name: Option<
                                        Box<dyn Fn(&S) -> BoxedCall<#request_type, #response_type> + Send + Sync>
                                    >
                                },
                                (true, false) => quote! {
                                    pub #method_name: Option<
                                        Box<dyn Fn(&S) -> BoxedStreamCall<#request_type, #response_type> + Send + Sync>
                                    >
                                },
                                (false, true) => quote! {
                                    pub #method_name: Option<
                                        Box<dyn Fn(&S) -> BoxedClientStreamCall<#request_type, #response_type> + Send + Sync>
                                    >
                                },
                                (true, true) => quote! {
                                    pub #method_name: Option<
                                        Box<dyn Fn(&S) -> BoxedBidiStreamCall<#request_type, #response_type> + Send + Sync>
                                    >
                                },
                            }
                        },
                    )
                    .collect();

            // Generate tonic server handler fields
            let tonic_server_handler_fields: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, is_cs, _, _)| {
                            match (*is_ss, *is_cs) {
                                (false, false) => quote! {
                                    pub #method_name: Option<BoxedCall<#request_type, #response_type>>
                                },
                                (true, false) => quote! {
                                    pub #method_name: Option<BoxedStreamCall<#request_type, #response_type>>
                                },
                                (false, true) => quote! {
                                    pub #method_name: Option<BoxedClientStreamCall<#request_type, #response_type>>
                                },
                                (true, true) => quote! {
                                    pub #method_name: Option<BoxedBidiStreamCall<#request_type, #response_type>>
                                },
                            }
                        },
                    )
                    .collect();

            // Generate final tonic service handler fields
            let tonic_service_handler_fields: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, is_cs, _, _)| {
                            match (*is_ss, *is_cs) {
                                (false, false) => quote! {
                                    #method_name: connectrpc_axum::tonic::BoxedCall<#request_type, #response_type>
                                },
                                (true, false) => quote! {
                                    #method_name: connectrpc_axum::tonic::BoxedStreamCall<#request_type, #response_type>
                                },
                                (false, true) => quote! {
                                    #method_name: connectrpc_axum::tonic::BoxedClientStreamCall<#request_type, #response_type>
                                },
                                (true, true) => quote! {
                                    #method_name: connectrpc_axum::tonic::BoxedBidiStreamCall<#request_type, #response_type>
                                },
                            }
                        },
                    )
                    .collect();

            // Generate field initializers for tonic builders
            let tonic_field_init: Vec<_> = method_info
                .iter()
                .map(
                    |(method_name, _request_type, _response_type, _path, _assoc, _ss, _is_cs, _, _)| {
                        quote! { #method_name: None }
                    },
                )
                .collect();

            // Generate handlers for build() with unimplemented fallbacks - no state version
            let tonic_build_handlers_no_state: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, is_cs, _, _)| {
                            match (*is_ss, *is_cs) {
                                (false, false) => quote! {
                                    let #method_name: BoxedCall<#request_type, #response_type> =
                                        self.#method_name
                                            .map(|mk| mk(&()))
                                            .unwrap_or_else(|| unimplemented_boxed_call());
                                },
                                (true, false) => quote! {
                                    let #method_name: BoxedStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .map(|mk| mk(&()))
                                            .unwrap_or_else(|| unimplemented_boxed_stream_call());
                                },
                                (false, true) => quote! {
                                    let #method_name: BoxedClientStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .map(|mk| mk(&()))
                                            .unwrap_or_else(|| unimplemented_boxed_client_stream_call());
                                },
                                (true, true) => quote! {
                                    let #method_name: BoxedBidiStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .map(|mk| mk(&()))
                                            .unwrap_or_else(|| unimplemented_boxed_bidi_stream_call());
                                },
                            }
                        },
                    )
                    .collect();

            // Generate handlers for build() with unimplemented fallbacks - with state version
            let tonic_build_handlers_with_state: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, _assoc, is_ss, is_cs, _, _)| {
                            match (*is_ss, *is_cs) {
                                (false, false) => quote! {
                                    let #method_name: BoxedCall<#request_type, #response_type> =
                                        self.#method_name
                                            .unwrap_or_else(|| unimplemented_boxed_call());
                                },
                                (true, false) => quote! {
                                    let #method_name: BoxedStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .unwrap_or_else(|| unimplemented_boxed_stream_call());
                                },
                                (false, true) => quote! {
                                    let #method_name: BoxedClientStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .unwrap_or_else(|| unimplemented_boxed_client_stream_call());
                                },
                                (true, true) => quote! {
                                    let #method_name: BoxedBidiStreamCall<#request_type, #response_type> =
                                        self.#method_name
                                            .unwrap_or_else(|| unimplemented_boxed_bidi_stream_call());
                                },
                            }
                        },
                    )
                    .collect();

            // Generate field names for with_state mapping
            let with_state_field_mapping: Vec<_> = method_info
                .iter()
                .map(
                    |(method_name, _request_type, _response_type, _path, _assoc, _ss, _is_cs, _, _)| {
                        quote! { #method_name: self.#method_name.map(|mk| mk(&state)) }
                    },
                )
                .collect();

            // Generate field names for final service creation
            let service_field_names: Vec<_> = method_info
                .iter()
                .map(
                    |(method_name, _request_type, _response_type, _path, _assoc, _ss, _is_cs, _, _)| {
                        quote! { #method_name }
                    },
                )
                .collect();

            // Generate tonic trait associated types for streaming response methods (server-streaming and bidi)
            let tonic_assoc_types: Vec<_> = method_info
                    .iter()
                    .filter_map(|(_method_name, _req, resp, _path, assoc, is_ss, _is_cs, _, _)| {
                        // Both server streaming and bidi streaming have response streams
                        if *is_ss {
                            Some(quote! {
                                type #assoc = std::pin::Pin<Box<dyn ::futures::Stream<Item = Result<#resp, ::tonic::Status>> + Send + 'static>>;
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

            // Generate tonic trait method impls for all streaming types
            // Each method builds a RequestContext from CapturedParts and tonic extensions
            let tonic_trait_methods: Vec<_> = method_info
                    .iter()
                    .map(
                        |(method_name, request_type, response_type, _path, assoc, is_ss, is_cs, _, _)| {
                            match (*is_ss, *is_cs) {
                                (false, false) => {
                                    // Unary: Request<Req> -> Response<Resp>
                                    quote! {
                                        async fn #method_name(
                                            &self,
                                            request: ::tonic::Request<#request_type>,
                                        ) -> Result<::tonic::Response<#response_type>, ::tonic::Status> {
                                            // Check if captured parts exist (FromRequestPartsLayer middleware applied)
                                            let captured = request.extensions()
                                                .get::<connectrpc_axum::tonic::CapturedParts>()
                                                .cloned();

                                            // Decompose tonic request - takes OWNERSHIP of extensions
                                            let (_metadata, extensions, inner) = request.into_parts();

                                            // Build Option<RequestContext> - None if middleware not applied
                                            // Handlers without extractors work fine with None
                                            // Handlers with extractors will return an error
                                            let ctx = captured.map(|captured| connectrpc_axum::tonic::RequestContext {
                                                method: captured.method,
                                                uri: captured.uri,
                                                version: captured.version,
                                                headers: captured.headers,
                                                extensions,
                                            });

                                            let req = connectrpc_axum::message::ConnectRequest(inner);
                                            match (self.#method_name)(ctx, req).await {
                                                Ok(response) => Ok(::tonic::Response::new(response.into_inner())),
                                                Err(err) => Err(err.into()),
                                            }
                                        }
                                    }
                                }
                                (true, false) => {
                                    // Server streaming: Request<Req> -> Response<Self::MethodStream>
                                    quote! {
                                        async fn #method_name(
                                            &self,
                                            request: ::tonic::Request<#request_type>,
                                        ) -> Result<::tonic::Response<Self::#assoc>, ::tonic::Status> {
                                            // Check if captured parts exist (FromRequestPartsLayer middleware applied)
                                            let captured = request.extensions()
                                                .get::<connectrpc_axum::tonic::CapturedParts>()
                                                .cloned();

                                            // Decompose tonic request - takes OWNERSHIP of extensions
                                            let (_metadata, extensions, inner) = request.into_parts();

                                            // Build Option<RequestContext> - None if middleware not applied
                                            // Handlers without extractors work fine with None
                                            // Handlers with extractors will return an error
                                            let ctx = captured.map(|captured| connectrpc_axum::tonic::RequestContext {
                                                method: captured.method,
                                                uri: captured.uri,
                                                version: captured.version,
                                                headers: captured.headers,
                                                extensions,
                                            });

                                            let req = connectrpc_axum::message::ConnectRequest(inner);
                                            match (self.#method_name)(ctx, req).await {
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
                                }
                                (false, true) => {
                                    // Client streaming: Request<Streaming<Req>> -> Response<Resp>
                                    quote! {
                                        async fn #method_name(
                                            &self,
                                            request: ::tonic::Request<::tonic::Streaming<#request_type>>,
                                        ) -> Result<::tonic::Response<#response_type>, ::tonic::Status> {
                                            // Check if captured parts exist (FromRequestPartsLayer middleware applied)
                                            let captured = request.extensions()
                                                .get::<connectrpc_axum::tonic::CapturedParts>()
                                                .cloned();

                                            // Decompose tonic request - takes OWNERSHIP of extensions
                                            let (_metadata, extensions, tonic_stream) = request.into_parts();

                                            // Build Option<RequestContext> - None if middleware not applied
                                            // Handlers without extractors work fine with None
                                            // Handlers with extractors will return an error
                                            let ctx = captured.map(|captured| connectrpc_axum::tonic::RequestContext {
                                                method: captured.method,
                                                uri: captured.uri,
                                                version: captured.version,
                                                headers: captured.headers,
                                                extensions,
                                            });

                                            // Convert tonic::Streaming to connectrpc_axum::Streaming
                                            let streaming = connectrpc_axum::message::Streaming::from_tonic(tonic_stream);
                                            let req = connectrpc_axum::message::ConnectRequest(streaming);
                                            match (self.#method_name)(ctx, req).await {
                                                Ok(response) => Ok(::tonic::Response::new(response.into_inner())),
                                                Err(err) => Err(err.into()),
                                            }
                                        }
                                    }
                                }
                                (true, true) => {
                                    // Bidi streaming: Request<Streaming<Req>> -> Response<Self::MethodStream>
                                    quote! {
                                        async fn #method_name(
                                            &self,
                                            request: ::tonic::Request<::tonic::Streaming<#request_type>>,
                                        ) -> Result<::tonic::Response<Self::#assoc>, ::tonic::Status> {
                                            // Check if captured parts exist (FromRequestPartsLayer middleware applied)
                                            let captured = request.extensions()
                                                .get::<connectrpc_axum::tonic::CapturedParts>()
                                                .cloned();

                                            // Decompose tonic request - takes OWNERSHIP of extensions
                                            let (_metadata, extensions, tonic_stream) = request.into_parts();

                                            // Build Option<RequestContext> - None if middleware not applied
                                            // Handlers without extractors work fine with None
                                            // Handlers with extractors will return an error
                                            let ctx = captured.map(|captured| connectrpc_axum::tonic::RequestContext {
                                                method: captured.method,
                                                uri: captured.uri,
                                                version: captured.version,
                                                headers: captured.headers,
                                                extensions,
                                            });

                                            // Convert tonic::Streaming to connectrpc_axum::Streaming
                                            let streaming = connectrpc_axum::message::Streaming::from_tonic(tonic_stream);
                                            let req = connectrpc_axum::message::ConnectRequest(streaming);
                                            match (self.#method_name)(ctx, req).await {
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
                    /// Returns the router and a gRPC service. Use [`MakeServiceBuilder`] to
                    /// apply [`ConnectLayer`] and combine with other services.
                    ///
                    /// [`MakeServiceBuilder`]: connectrpc_axum::MakeServiceBuilder
                    /// [`ConnectLayer`]: connectrpc_axum::ConnectLayer
                    pub fn build(self) -> (
                        axum::Router,
                        #server_mod_name::#tonic_server_type_name<#tonic_service_name>
                    ) {
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
                    pub fn build(self) -> (
                        axum::Router,
                        #server_mod_name::#tonic_server_type_name<#tonic_service_name>
                    ) {
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

            // Determine which streaming types are actually used by this service
            let has_unary = method_info
                .iter()
                .any(|(_, _, _, _, _, is_ss, is_cs, _, _)| !*is_ss && !*is_cs);
            let has_server_stream = method_info
                .iter()
                .any(|(_, _, _, _, _, is_ss, is_cs, _, _)| *is_ss && !*is_cs);
            let has_client_stream = method_info
                .iter()
                .any(|(_, _, _, _, _, is_ss, is_cs, _, _)| !*is_ss && *is_cs);
            let has_bidi_stream = method_info
                .iter()
                .any(|(_, _, _, _, _, is_ss, is_cs, _, _)| *is_ss && *is_cs);

            // Only generate type aliases and helper functions for streaming types actually used
            let boxed_call_alias = if has_unary {
                quote! {
                    type BoxedCall<Req, Resp> = connectrpc_axum::tonic::BoxedCall<Req, Resp>;
                }
            } else {
                quote! {}
            };

            let boxed_stream_call_alias = if has_server_stream {
                quote! {
                    type BoxedStreamCall<Req, Resp> = connectrpc_axum::tonic::BoxedStreamCall<Req, Resp>;
                }
            } else {
                quote! {}
            };

            let boxed_client_stream_call_alias = if has_client_stream {
                quote! {
                    type BoxedClientStreamCall<Req, Resp> = connectrpc_axum::tonic::BoxedClientStreamCall<Req, Resp>;
                }
            } else {
                quote! {}
            };

            let boxed_bidi_stream_call_alias = if has_bidi_stream {
                quote! {
                    type BoxedBidiStreamCall<Req, Resp> = connectrpc_axum::tonic::BoxedBidiStreamCall<Req, Resp>;
                }
            } else {
                quote! {}
            };

            let unimplemented_boxed_call_fn = if has_unary {
                quote! {
                    fn unimplemented_boxed_call<Req, Resp>() -> BoxedCall<Req, Resp>
                    where
                        Req: Send + Sync + 'static,
                        Resp: Send + Sync + 'static,
                    {
                        connectrpc_axum::tonic::unimplemented_boxed_call::<Req, Resp>()
                    }
                }
            } else {
                quote! {}
            };

            let unimplemented_boxed_stream_call_fn = if has_server_stream {
                quote! {
                    fn unimplemented_boxed_stream_call<Req, Resp>() -> BoxedStreamCall<Req, Resp>
                    where
                        Req: Send + Sync + 'static,
                        Resp: Send + Sync + 'static,
                    {
                        connectrpc_axum::tonic::unimplemented_boxed_stream_call::<Req, Resp>()
                    }
                }
            } else {
                quote! {}
            };

            let unimplemented_boxed_client_stream_call_fn = if has_client_stream {
                quote! {
                    fn unimplemented_boxed_client_stream_call<Req, Resp>() -> BoxedClientStreamCall<Req, Resp>
                    where
                        Req: Send + Sync + 'static,
                        Resp: Send + Sync + 'static,
                    {
                        connectrpc_axum::tonic::unimplemented_boxed_client_stream_call::<Req, Resp>()
                    }
                }
            } else {
                quote! {}
            };

            let unimplemented_boxed_bidi_stream_call_fn = if has_bidi_stream {
                quote! {
                    fn unimplemented_boxed_bidi_stream_call<Req, Resp>() -> BoxedBidiStreamCall<Req, Resp>
                    where
                        Req: Send + Sync + 'static,
                        Resp: Send + Sync + 'static,
                    {
                        connectrpc_axum::tonic::unimplemented_boxed_bidi_stream_call::<Req, Resp>()
                    }
                }
            } else {
                quote! {}
            };

            let module_bits = quote! {
                // Local aliases to reduce fully-qualified verbosity in generated code
                // Only include type aliases for streaming types actually used by this service
                #boxed_call_alias
                #boxed_stream_call_alias
                #boxed_client_stream_call_alias
                #boxed_bidi_stream_call_alias

                #unimplemented_boxed_call_fn
                #unimplemented_boxed_stream_call_fn
                #unimplemented_boxed_client_stream_call_fn
                #unimplemented_boxed_bidi_stream_call_fn

                #tonic_builder_structs
            };

            let out_of_module = quote! {
                /// Generated Tonic-compatible service that holds boxed calls.
                /// This struct directly implements the Tonic trait, following Tonic's idiomatic
                /// approach where the trait serves as the primary interface.
                #[allow(dead_code)]
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
            #[allow(dead_code)]
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

                    /// Build with default [`ConnectLayer`] applied.
                    ///
                    /// This is a convenience method that applies a default `ConnectLayer`
                    /// to the router. For custom configuration, use [`build()`] and apply
                    /// the layer manually.
                    ///
                    /// [`ConnectLayer`]: connectrpc_axum::ConnectLayer
                    pub fn build_connect(self) -> axum::Router<()> {
                        self.router.layer(connectrpc_axum::ConnectLayer::new())
                    }
                }

                #tonic_module_bits
            }

            #tonic_out_of_module
        };

        buf.push_str(&routes_fn.to_string());
    }
}

#[cfg(test)]
mod tests;
