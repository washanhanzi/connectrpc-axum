mod client;
mod tonic;

use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use prost_types::method_options::IdempotencyLevel;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::io::Result;

use crate::merge::append_generated_section;
use crate::schema::{SchemaSet, ServiceModel};

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

fn rust_ident(name: &str) -> Ident {
    if let Some(raw) = name.strip_prefix("r#") {
        return Ident::new_raw(raw, Span::call_site());
    }

    format_ident!("{}", name)
}

fn ident_base_name(ident: &Ident) -> String {
    ident.to_string().trim_start_matches("r#").to_string()
}

fn ident_base_str(name: &str) -> &str {
    name.trim_start_matches("r#")
}

pub(super) fn derived_method_ident(method_name: &Ident, suffix: &str) -> Ident {
    format_ident!("{}_{}", ident_base_name(method_name), suffix)
}

pub(super) fn method_const_ident(method_name: &Ident) -> Ident {
    format_ident!("{}", ident_base_name(method_name).to_uppercase())
}

pub(super) fn prefixed_method_ident(prefix: &str, method_name: &Ident) -> Ident {
    format_ident!("{}_{}", prefix, ident_base_name(method_name))
}

#[derive(Debug, Clone)]
pub(super) struct ServiceInfo {
    pub name: String,
}

#[derive(Debug, Clone)]
pub(super) struct MethodInfo {
    pub method_name: Ident,
    pub request_type: TokenStream,
    pub response_type: TokenStream,
    pub path: String,
    pub stream_assoc: Ident,
    pub server_streaming: bool,
    pub client_streaming: bool,
    pub idempotency_level: Option<i32>,
    pub idempotency_tokens: TokenStream,
}

#[derive(Default)]
pub struct AxumConnectServiceGenerator {
    include_connect_server: bool,
    include_tonic: bool,
    include_connect_client: bool,
}

impl AxumConnectServiceGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_connect_server(mut self, include: bool) -> Self {
        self.include_connect_server = include;
        self
    }

    pub fn with_tonic(mut self, include: bool) -> Self {
        self.include_tonic = include;
        self
    }

    pub fn with_connect_client(mut self, include: bool) -> Self {
        self.include_connect_client = include;
        self
    }

    pub fn append_to_out_dir(&self, schema: &SchemaSet, out_dir: &str) -> Result<()> {
        let mut generated_by_file = BTreeMap::<String, String>::new();

        for service in &schema.services {
            let generated = self.generate_service(schema, service)?;
            if generated.is_empty() {
                continue;
            }

            let file_stem = if service.package.is_empty() {
                "_".to_string()
            } else {
                service.package.clone()
            };
            generated_by_file
                .entry(file_stem)
                .or_default()
                .push_str(&generated);
        }

        for (file_stem, generated) in generated_by_file {
            let path = format!("{out_dir}/{file_stem}.rs");
            if !std::path::Path::new(&path).exists() {
                println!(
                    "cargo:warning=Skipping generated Connect code for '{}': no matching prost output file.",
                    file_stem
                );
                continue;
            }

            append_generated_section(
                std::path::Path::new(&path),
                "// --- Connect service/client code ---",
                &generated,
            )?;
        }

        Ok(())
    }

    fn generate_service(&self, schema: &SchemaSet, service: &ServiceModel) -> Result<String> {
        let prost = schema.prost();
        let service_name = prost.rust_service_name(&service.proto_name);
        let service_info = ServiceInfo {
            name: service_name.clone(),
        };

        // Server module name (e.g., hello_world_service_connect)
        let service_module_name = format_ident!(
            "{}_connect",
            service
                .proto_name
                .to_case(Case::Snake)
                .trim_start_matches("r#")
        );

        // Remove "Service" suffix from the logical service name to avoid duplication
        let service_base_name = ident_base_str(
            service_name
                .strip_suffix("Service")
                .unwrap_or(&service_name),
        );
        let service_builder_name = format_ident!("{}ServiceBuilder", service_base_name);

        let nested_method_info = build_method_info(schema, service, 1)?;
        let root_method_info = build_method_info(schema, service, 0)?;

        // Generate Connect-only builder methods for all RPC types.
        let connect_builder_methods: Vec<_> = nested_method_info
            .iter()
            .map(|method| {
                let is_unary = !method.server_streaming && !method.client_streaming;
                let supports_get = is_unary && is_no_side_effects(method.idempotency_level);

                let doc = match (
                    method.server_streaming,
                    method.client_streaming,
                    supports_get,
                ) {
                    (false, false, true) => {
                        "Register a handler for this RPC method (unary, GET+POST enabled)"
                    }
                    (false, false, false) => "Register a handler for this RPC method (unary)",
                    (true, false, _) => {
                        "Register a handler for this RPC method (server streaming)"
                    }
                    (false, true, _) => {
                        "Register a handler for this RPC method (client streaming)"
                    }
                    (true, true, _) => {
                        "Register a handler for this RPC method (bidirectional streaming)"
                    }
                };

                let idempotency_const_name = format_ident!(
                    "{}_IDEMPOTENCY",
                    ident_base_name(&method.method_name).to_uppercase()
                );

                let request_type = &method.request_type;
                let response_type = &method.response_type;
                let method_name = &method.method_name;
                let path = &method.path;
                let idempotency_tokens = &method.idempotency_tokens;

                let method_router_expr = if supports_get {
                    quote! {
                        connectrpc_axum::handler::get_connect::<
                            F,
                            T,
                            S,
                            #request_type,
                            #response_type,
                        >(handler.clone())
                        .merge(connectrpc_axum::handler::post_connect::<
                            F,
                            T,
                            S,
                            #request_type,
                            #response_type,
                        >(handler))
                    }
                } else {
                    quote! {
                        connectrpc_axum::handler::post_connect::<
                            F,
                            T,
                            S,
                            #request_type,
                            #response_type,
                        >(handler)
                    }
                };

                quote! {
                    /// Idempotency level for this RPC method.
                    #[allow(dead_code)]
                    pub const #idempotency_const_name: connectrpc_axum::IdempotencyLevel = #idempotency_tokens;

                    #[doc = #doc]
                    pub fn #method_name<F, T>(self, handler: F) -> #service_builder_name<S>
                    where
                        connectrpc_axum::handler::ConnectHandlerWrapper<
                            F,
                            #request_type,
                            #response_type,
                        >: axum::handler::Handler<T, S>,
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

        let (tonic_module_bits, tonic_out_of_module) = if self.include_tonic {
            tonic::generate_tonic_code(
                &service_info,
                &nested_method_info,
                &root_method_info,
                service_base_name,
            )
        } else {
            (quote! {}, quote! {})
        };

        let mut buf = String::new();

        if self.include_connect_server || self.include_tonic {
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
        }

        if self.include_connect_client {
            let client_code = client::generate_connect_client(&service_info, &nested_method_info);
            buf.push_str(&client_code.to_string());
        }

        Ok(buf)
    }
}

fn build_method_info(
    schema: &SchemaSet,
    service: &ServiceModel,
    nesting: usize,
) -> Result<Vec<MethodInfo>> {
    let prost = schema.prost();

    service
        .methods
        .iter()
        .map(|method| {
            let method_name = rust_ident(&prost.rust_method_name(&method.proto_name));
            let request_type = prost
                .rust_type_relative(&method.input_type, &service.package, nesting)
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "unable to resolve input type '{}' for {}.{}",
                        method.input_type, service.proto_name, method.proto_name
                    ))
                })?
                .parse()
                .map_err(|e| {
                    std::io::Error::other(format!(
                        "invalid request type path '{}': {e}",
                        method.input_type
                    ))
                })?;
            let response_type = prost
                .rust_type_relative(&method.output_type, &service.package, nesting)
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "unable to resolve output type '{}' for {}.{}",
                        method.output_type, service.proto_name, method.proto_name
                    ))
                })?
                .parse()
                .map_err(|e| {
                    std::io::Error::other(format!(
                        "invalid response type path '{}': {e}",
                        method.output_type
                    ))
                })?;
            let stream_assoc = format_ident!("{}Stream", method.proto_name);
            let idempotency_level = method.idempotency_level;
            let idempotency_tokens = idempotency_level_tokens(idempotency_level);

            Ok(MethodInfo {
                method_name,
                request_type,
                response_type,
                path: method.route_path.clone(),
                stream_assoc,
                server_streaming: method.server_streaming,
                client_streaming: method.client_streaming,
                idempotency_level,
                idempotency_tokens,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
