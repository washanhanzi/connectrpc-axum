use convert_case::{Case, Casing};
use prost_build::{Service, ServiceGenerator};
use quote::{format_ident, quote};

#[derive(Default)]
pub struct AxumConnectServiceGenerator {
    pub generate_grpc_adapter: bool,
}

impl AxumConnectServiceGenerator {
    pub fn new() -> Self {
        Self {
            generate_grpc_adapter: false,
        }
    }

    pub fn with_grpc_adapter(mut self, enabled: bool) -> Self {
        self.generate_grpc_adapter = enabled;
        self
    }
}

impl ServiceGenerator for AxumConnectServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        let service_module_name = format_ident!("{}", service.name.to_lowercase());
        let _handlers_struct_name = format_ident!("{}Handlers", service.name);

        // Generate route registrations that accept any ConnectHandler implementation
        // This provides maximum flexibility for handler parameters while ensuring
        // type safety through the ConnectHandler trait constraint

        // Generate routes that accept ConnectHandler implementations wrapped in ConnectHandlerWrapper
        let routes: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                let path = format!(
                    "/{}.{}/{}",
                    service.package, service.proto_name, method.proto_name
                );
                let method_name = format_ident!("{}", method.name.to_case(Case::Snake));

                quote! {
                    .route(
                        #path,
                        axum::routing::post(connectrpc_axum::handler::ConnectHandlerWrapper(handlers.#method_name))
                    )
                }
            })
            .collect();

        // Generate handler struct fields
        let handler_fields: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                let method_name = format_ident!("{}", method.name.to_case(Case::Snake));
                let handler_type = format_ident!("{}Handler", method.name.to_case(Case::Pascal));

                quote! {
                    pub #method_name: #handler_type
                }
            })
            .collect();

        // Generate handler type parameters
        let handler_type_params: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                let handler_type = format_ident!("{}Handler", method.name.to_case(Case::Pascal));

                quote! {
                    #handler_type
                }
            })
            .collect();

        // Generate handler where clauses - require ConnectHandler which provides Handler via bridge
        let handler_where_clauses: Vec<_> = service.methods.iter().map(|method| {
            let handler_type = format_ident!("{}Handler", method.name.to_case(Case::Pascal));
            let method_name = format_ident!("{}", method.name.to_case(Case::Snake));
            let tuple_type_param = format_ident!("T{}", method.name.to_case(Case::Pascal));
            
            // For ConnectHandler, we need to specify the parameter tuple type
            // This will be something like (State<S>, ConnectRequest<HelloRequest>) or (ConnectRequest<HelloRequest>)
            quote! {
                #handler_type: connectrpc_axum::handler::ConnectHandler<#tuple_type_param, S> + Send + Sync + 'static,
                #tuple_type_param: Send + 'static
            }
        }).collect();

        // Generate type parameters for handler parameter tuples
        let handler_tuple_type_params: Vec<_> = service.methods.iter().map(|method| {
            let tuple_type_param = format_ident!("T{}", method.name.to_case(Case::Pascal));
            quote! {
                #tuple_type_param
            }
        }).collect();

        let handlers_struct_name = format_ident!("{}Handlers", service.name);

        let routes_fn = quote! {
            pub mod #service_module_name {
                use super::*;

                /// Handlers struct for #service.name service.
                /// Each field represents a handler for one RPC method.
                /// Handlers can be any type implementing ConnectHandler with the appropriate signature.
                #[derive(Clone)]
                pub struct #handlers_struct_name<#(#handler_type_params,)*> {
                    #(#handler_fields,)*
                }

                /// Create a router for the #service.name service.
                ///
                /// Takes a handlers struct containing ConnectHandler implementations
                /// and returns a Router<S> that can be merged into your main application router.
                ///
                /// # Example
                /// ```rust,no_run
                /// use axum::Router;
                ///
                /// async fn say_hello(
                ///     ConnectRequest(req): ConnectRequest<HelloRequest>
                /// ) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
                ///     // handler implementation
                /// }
                ///
                /// let handlers = #handlers_struct_name {
                ///     say_hello,
                /// };
                ///
                /// let app = Router::new()
                ///     .merge(#service_module_name::router(handlers))
                ///     .with_state(app_state);
                /// ```
                pub fn router<S, #(#handler_type_params,)* #(#handler_tuple_type_params,)*>(
                    handlers: #handlers_struct_name<#(#handler_type_params,)*>
                ) -> axum::Router<S>
                where
                    S: Clone + Send + Sync + 'static,
                    #(#handler_where_clauses,)*
                {
                    axum::Router::new()
                        #(#routes)*
                }
            }
        };

        buf.push_str(&routes_fn.to_string());

        // Generate gRPC adapter if enabled
        if self.generate_grpc_adapter {
            let grpc_adapter = self.generate_grpc_adapter(
                &service,
                &handlers_struct_name,
                &handler_type_params,
                &handler_where_clauses,
                &service_module_name
            );
            buf.push_str(&grpc_adapter);
        }
    }
}

impl AxumConnectServiceGenerator {
    fn generate_grpc_adapter(
        &self,
        service: &prost_build::Service,
        handlers_struct_name: &proc_macro2::Ident,
        handler_type_params: &[proc_macro2::TokenStream],
        handler_where_clauses: &[proc_macro2::TokenStream],
        service_module_name: &proc_macro2::Ident,
    ) -> String {
        let service_module_name = format_ident!("{}", service.name.to_lowercase());
        let handlers_trait_name = format_ident!("{}Handlers", service.name);
        let grpc_service_trait_name = format_ident!("{}", service.name);
        let grpc_server_name = format_ident!("{}Server", service.name);

        // Generate error conversion function
        let error_conversion = quote! {
            /// Convert ConnectError to tonic::Status
            fn convert_connect_error_to_status(error: connectrpc_axum::error::ConnectError) -> tonic::Status {
                use connectrpc_axum::error::Code;

                let tonic_code = match error.code() {
                    Code::Canceled => tonic::Code::Cancelled,
                    Code::Unknown => tonic::Code::Unknown,
                    Code::InvalidArgument => tonic::Code::InvalidArgument,
                    Code::DeadlineExceeded => tonic::Code::DeadlineExceeded,
                    Code::NotFound => tonic::Code::NotFound,
                    Code::AlreadyExists => tonic::Code::AlreadyExists,
                    Code::PermissionDenied => tonic::Code::PermissionDenied,
                    Code::ResourceExhausted => tonic::Code::ResourceExhausted,
                    Code::FailedPrecondition => tonic::Code::FailedPrecondition,
                    Code::Aborted => tonic::Code::Aborted,
                    Code::OutOfRange => tonic::Code::OutOfRange,
                    Code::Unimplemented => tonic::Code::Unimplemented,
                    Code::Internal => tonic::Code::Internal,
                    Code::Unavailable => tonic::Code::Unavailable,
                    Code::DataLoss => tonic::Code::DataLoss,
                    Code::Unauthenticated => tonic::Code::Unauthenticated,
                    Code::Ok => tonic::Code::Ok,
                };

                let message = error.message().unwrap_or("").to_string();
                tonic::Status::new(tonic_code, message)
            }
        };

        // Generate GrpcAdapter struct that works with the new handler struct system
        let grpc_adapter_struct = quote! {
            /// Auto-generated gRPC adapter that wraps ConnectRPC handlers
            #[derive(Clone)]
            pub struct GrpcAdapter<#(#handler_type_params,)* S>
            where
                S: Clone + Send + Sync + 'static,
                #(#handler_where_clauses,)*
            {
                handlers: super::#service_module_name::#handlers_struct_name<#(#handler_type_params,)*>,
                state: S,
            }
        };

        // Add request/response conversion helpers
        let conversion_helpers = quote! {
            /// Convert tonic::Request to Axum Request for ConnectRequest extraction
            fn tonic_to_axum_request<T>(tonic_req: tonic::Request<T>) -> axum::extract::Request
            where
                T: prost::Message + serde::Serialize,
            {
                let body = ::serde_json::to_vec(&tonic_req.into_inner()).unwrap();
                axum::extract::Request::builder()
                    .method("POST")
                    .uri("/")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap()
            }

            /// Extract response data from Axum Response
            async fn extract_response_data<T>(response: axum::response::Response) -> Result<T, tonic::Status>
            where
                T: serde::de::DeserializeOwned,
            {
                let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await
                    .map_err(|e| tonic::Status::internal(format!("Failed to read response body: {}", e)))?;
                
                ::serde_json::from_slice::<T>(&body_bytes)
                    .map_err(|e| tonic::Status::internal(format!("Failed to parse response: {}", e)))
            }
        };

        // Generate gRPC service trait methods using Handler::call
        let grpc_trait_methods = service.methods.iter().map(|method| {
            let method_name = format_ident!("{}", method.name.to_case(Case::Snake));
            let request_ty = format_ident!("{}", method.input_type.split('.').last().unwrap_or(&method.input_type));
            let response_ty = format_ident!("{}", method.output_type.split('.').last().unwrap_or(&method.output_type));

            if method.server_streaming {
                let stream_type_name = format_ident!("{}Stream", method.name.to_case(Case::Pascal));
                quote! {
                    type #stream_type_name = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<super::#response_ty, tonic::Status>> + Send>>;
                    
                    async fn #method_name(
                        &self,
                        request: tonic::Request<super::#request_ty>,
                    ) -> Result<tonic::Response<Self::#stream_type_name>, tonic::Status> {
                        // Convert tonic::Request to Axum Request
                        let axum_request = tonic_to_axum_request(request);
                        
                        // Call the handler using Axum's Handler trait
                        // This works for both stateless and stateful handlers automatically
                        let response = axum::handler::Handler::call(
                            self.handlers.#method_name.clone(),
                            axum_request,
                            self.state.clone()
                        ).await;

                        // For streaming, we need to extract the stream and convert it
                        // This is complex and requires proper stream handling
                        // For now, return an error to indicate this needs implementation
                        Err(tonic::Status::unimplemented("Server streaming not yet implemented in gRPC adapter"))
                    }
                }
            } else {
                quote! {
                    async fn #method_name(
                        &self,
                        request: tonic::Request<super::#request_ty>,
                    ) -> Result<tonic::Response<super::#response_ty>, tonic::Status> {
                        // Convert tonic::Request to Axum Request  
                        let axum_request = tonic_to_axum_request(request);
                        
                        // Call the handler using Axum's Handler trait
                        // This works for both stateless and stateful handlers automatically
                        let response = axum::handler::Handler::call(
                            self.handlers.#method_name.clone(),
                            axum_request,
                            self.state.clone()
                        ).await;
                        
                        // Extract the response data
                        let response_data = extract_response_data::<super::#response_ty>(response).await?;
                        
                        Ok(tonic::Response::new(response_data))
                    }
                }
            }
        });

        // Generate trait implementation (using native async fn in traits - no async_trait needed)
        let grpc_trait_impl = quote! {
            impl<#(#handler_type_params,)* S> super::hello_world_service_server::#grpc_service_trait_name for GrpcAdapter<#(#handler_type_params,)* S>
            where
                S: Clone + Send + Sync + 'static,
                #(#handler_where_clauses,)*
            {
                #(#grpc_trait_methods)*
            }
        };

        // Generate helper function to create both services
        let helper_function = quote! {
            /// Create both ConnectRPC router and gRPC service from a single handler struct
            ///
            /// This allows serving both protocols with the same business logic.
            /// Note: gRPC handlers support only State<S> and ConnectRequest<T> parameters.
            /// Additional extractors (Query, Path, etc.) will cause compilation errors.
            ///
            /// # Example
            /// ```rust,no_run
            /// let handlers = HelloWorldServiceHandlers {
            ///     say_hello,        // fn(ConnectRequest<Req>) -> Result<ConnectResponse<Res>, ConnectError>
            ///     say_hello_stream, // fn(State<S>, ConnectRequest<Req>) -> ConnectStreamResponse<...>
            /// };
            /// let (connect_router, grpc_service) = helloworldservice::grpc_adapter::create_services(
            ///     handlers,
            ///     app_state
            /// );
            /// ```
            pub fn create_services<#(#handler_type_params,)* S>(
                handlers: super::#service_module_name::#handlers_struct_name<#(#handler_type_params,)*>,
                state: S
            ) -> (axum::Router<S>, super::hello_world_service_server::#grpc_server_name<GrpcAdapter<#(#handler_type_params,)* S>>)
            where
                S: Clone + Send + Sync + 'static,
                #(#handler_where_clauses,)*
            {
                let connect_router = super::#service_module_name::router(handlers.clone()).with_state(state.clone());
                let grpc_adapter = GrpcAdapter { 
                    handlers: handlers.clone(),
                    state: state.clone() 
                };
                let grpc_service = super::hello_world_service_server::#grpc_server_name::new(grpc_adapter);
                (connect_router, grpc_service)
            }
        };

        let complete_grpc_module = quote! {

            #error_conversion

            pub mod grpc_adapter {
                use super::*;

                #conversion_helpers

                #grpc_adapter_struct

                #grpc_trait_impl

                #helper_function
            }
        };

        format!("\n{}", complete_grpc_module)
    }
}
