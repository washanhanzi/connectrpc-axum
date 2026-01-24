mod client;
mod tonic;

use convert_case::{Case, Casing};
use prost_build::{Service, ServiceGenerator};
use prost_types::method_options::IdempotencyLevel;
use quote::{format_ident, quote};

use client::MethodInfo;

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
    include_connect_client: bool,
}

impl AxumConnectServiceGenerator {
    pub fn with_tonic(include_tonic: bool) -> Self {
        Self {
            include_tonic,
            include_connect_client: false,
        }
    }

    pub fn with_connect_client(mut self, include_connect_client: bool) -> Self {
        self.include_connect_client = include_connect_client;
        self
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
        let method_info: Vec<MethodInfo> = service
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
            tonic::generate_tonic_code(&service, &method_info, service_base_name)
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

                    /// Build with default layers applied via [`MakeServiceBuilder`].
                    ///
                    /// This is a convenience method that wraps the router with
                    /// `MakeServiceBuilder::new()` which provides:
                    /// - Default gzip compression/decompression
                    /// - [`ConnectLayer`] with default settings
                    ///
                    /// For custom configuration, use [`build()`] and configure
                    /// `MakeServiceBuilder` manually.
                    ///
                    /// [`MakeServiceBuilder`]: connectrpc_axum::MakeServiceBuilder
                    /// [`ConnectLayer`]: connectrpc_axum::ConnectLayer
                    pub fn build_connect(self) -> axum::Router<()> {
                        connectrpc_axum::MakeServiceBuilder::new()
                            .add_router(self.router)
                            .build()
                    }
                }

                #tonic_module_bits
            }

            #tonic_out_of_module
        };

        buf.push_str(&routes_fn.to_string());

        // Generate Connect client code if enabled
        if self.include_connect_client {
            let client_code = client::generate_connect_client(&service, &method_info);
            buf.push_str(&client_code.to_string());
        }
    }
}

#[cfg(test)]
mod tests;
