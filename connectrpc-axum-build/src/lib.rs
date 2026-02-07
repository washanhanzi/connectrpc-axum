use r#gen::AxumConnectServiceGenerator;
use std::io::Result;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

/// Code generation module for service builders.
mod r#gen;
mod include_file;

/// Source of proto files - either auto-discovered from a directory or explicit file list.
enum ProtoSource {
    /// Auto-discover all .proto files in this directory recursively.
    Directory(PathBuf),
    /// Explicit list of proto files and include directories.
    Files {
        protos: Vec<PathBuf>,
        includes: Vec<PathBuf>,
    },
}

// ============================================================================
// Type-state marker types
// ============================================================================

/// Marker indicating a feature is enabled.
pub struct Enabled;

/// Marker indicating a feature is disabled.
pub struct Disabled;

/// Marker indicating no proto source has been set yet.
pub struct NoSource;

/// Marker indicating a proto source has been set.
pub struct WithSource(ProtoSource);

/// Trait to convert type markers to runtime booleans.
pub trait BuildMarker {
    /// The boolean value this marker represents.
    const VALUE: bool;
}

impl BuildMarker for Enabled {
    const VALUE: bool = true;
}

impl BuildMarker for Disabled {
    const VALUE: bool = false;
}

/// Builder for compiling proto files with optional configuration.
///
/// Type parameters control code generation:
/// - `Source`: Whether a proto source has been provided (`NoSource` or `WithSource`)
/// - `Connect`: Whether to generate Connect service handlers
/// - `Tonic`: Whether to generate Tonic gRPC server stubs (requires `tonic` feature)
/// - `TonicClient`: Whether to generate Tonic gRPC client stubs (requires `tonic-client` feature)
/// - `ConnectClient`: Whether to generate typed Connect RPC client code
///
/// Use [`builder()`] to create a builder without a source, then call
/// [`compile_dir()`](CompileBuilder::compile_dir) or
/// [`compile_protos()`](CompileBuilder::compile_protos) to set the source.
/// Alternatively, use the free functions [`compile_dir()`](crate::compile_dir) or
/// [`compile_protos()`](crate::compile_protos) as shortcuts.
///
/// To compile multiple proto roots, use separate builders:
///
/// ```rust,ignore
/// connectrpc_axum_build::compile_dir("proto1")
///     .include_file("protos1.rs")
///     .compile()?;
/// connectrpc_axum_build::compile_dir("proto2")
///     .include_file("protos2.rs")
///     .compile()?;
/// ```
pub struct CompileBuilder<Source = NoSource, Connect = Enabled, Tonic = Disabled, TonicClient = Disabled, ConnectClient = Disabled> {
    source: Source,
    out_dir: Option<PathBuf>,
    include_file: Option<PathBuf>,
    extern_reexports: Vec<(String, String)>,
    #[cfg(feature = "fetch-protoc")]
    protoc_path: Option<PathBuf>,
    prost_config: Option<Box<dyn Fn(&mut prost_build::Config)>>,
    pbjson_config: Option<Box<dyn Fn(&mut pbjson_build::Builder)>>,
    #[cfg(feature = "tonic")]
    tonic_config: Option<Box<dyn Fn(tonic_prost_build::Builder) -> tonic_prost_build::Builder>>,
    #[cfg(feature = "tonic-client")]
    tonic_client_config:
        Option<Box<dyn Fn(tonic_prost_build::Builder) -> tonic_prost_build::Builder>>,
    _marker: PhantomData<(Connect, Tonic, TonicClient, ConnectClient)>,
}

// ============================================================================
// Methods to set proto source (only available when Source = NoSource)
// ============================================================================

impl<C, T, TC, CC> CompileBuilder<NoSource, C, T, TC, CC> {
    /// Set a directory of proto files to compile.
    ///
    /// Auto-discovers all `.proto` files in the directory recursively.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::builder()
    ///         .compile_dir("proto")
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn compile_dir(self, dir: impl AsRef<Path>) -> CompileBuilder<WithSource, C, T, TC, CC> {
        CompileBuilder {
            source: WithSource(ProtoSource::Directory(dir.as_ref().to_path_buf())),
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_reexports: self.extern_reexports,

            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            prost_config: self.prost_config,
            pbjson_config: self.pbjson_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }

    /// Set specific proto files to compile.
    ///
    /// # Arguments
    ///
    /// * `protos` - Proto files to compile
    /// * `includes` - Directories to search for imports
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::builder()
    ///         .compile_protos(&["proto/service.proto"], &["proto"])
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn compile_protos<P: AsRef<Path>>(
        self,
        protos: &[P],
        includes: &[P],
    ) -> CompileBuilder<WithSource, C, T, TC, CC> {
        CompileBuilder {
            source: WithSource(ProtoSource::Files {
                protos: protos.iter().map(|p| p.as_ref().to_path_buf()).collect(),
                includes: includes.iter().map(|p| p.as_ref().to_path_buf()).collect(),
            }),
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_reexports: self.extern_reexports,

            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            prost_config: self.prost_config,
            pbjson_config: self.pbjson_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Methods available when Connect = Enabled
// ============================================================================

impl<S, T, TC, CC> CompileBuilder<S, Enabled, T, TC, CC> {
    /// Skip generating Connect server code.
    ///
    /// When called, only message types and serde implementations are generated.
    /// No Connect service builders (e.g., `HelloWorldServiceBuilder`) will be created.
    ///
    /// **Note:** After calling this, `with_tonic()` is no longer available since
    /// tonic server stubs depend on the Connect service module.
    ///
    /// Use this when you only need:
    /// - Protobuf message types with JSON serialization
    /// - Connect client (via `with_connect_client()`)
    /// - Tonic client (via `with_tonic_client()`)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .no_connect_server()  // Skip server code
    ///         .with_connect_client() // Generate client code
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn no_connect_server(self) -> CompileBuilder<S, Disabled, Disabled, TC, Disabled> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_reexports: self.extern_reexports,

            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            prost_config: self.prost_config,
            pbjson_config: self.pbjson_config,
            #[cfg(feature = "tonic")]
            tonic_config: None,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Methods available when Connect = Enabled AND Tonic = Disabled (enable tonic)
// ============================================================================

#[cfg(feature = "tonic")]
impl<S, TC, CC> CompileBuilder<S, Enabled, Disabled, TC, CC> {
    /// Enable generating tonic gRPC server stubs (second pass) + tonic-compatible helpers in first pass.
    ///
    /// **Note:** After calling this, `no_connect_server()` is no longer available since
    /// tonic server stubs depend on the Connect service module.
    pub fn with_tonic(self) -> CompileBuilder<S, Enabled, Enabled, TC, CC> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_reexports: self.extern_reexports,

            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            prost_config: self.prost_config,
            pbjson_config: self.pbjson_config,
            tonic_config: self.tonic_config,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Methods available when Tonic = Enabled (configure tonic)
// ============================================================================

#[cfg(feature = "tonic")]
impl<S, C, TC, CC> CompileBuilder<S, C, Enabled, TC, CC> {
    /// Customize the tonic prost builder with a configuration closure.
    ///
    /// The closure is applied before the required internal configuration. Internal settings
    /// (like `build_client(false)`, `build_server(true)`, `compile_well_known_types(false)`,
    /// `out_dir`, and `extern_path` mappings) will be applied after and take precedence.
    ///
    /// **Important:** This only affects service trait generation, not message types.
    /// Message types are generated in Pass 1 and reused via `extern_path` in Pass 2.
    /// To customize message types, use [`with_prost_config`](Self::with_prost_config) instead.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_prost_config(|config| {
    ///             // Configure message types here (Pass 1)
    ///             config.type_attribute("MyMessage", "#[derive(Hash)]");
    ///         })
    ///         .with_tonic()
    ///         .with_tonic_prost_config(|builder| {
    ///             // Configure service generation here (Pass 2)
    ///             // Note: type_attribute for messages won't work here
    ///             builder
    ///         })
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn with_tonic_prost_config<F>(mut self, f: F) -> Self
    where
        F: Fn(tonic_prost_build::Builder) -> tonic_prost_build::Builder + 'static,
    {
        self.tonic_config = Some(Box::new(f));
        self
    }
}

// ============================================================================
// Methods available on all builder states (configuration)
// ============================================================================

impl<S, C, T, TC, CC> CompileBuilder<S, C, T, TC, CC> {
    /// Fetch and configure the protoc compiler.
    ///
    /// Downloads the specified version of protoc and sets the `PROTOC` environment
    /// variable so that prost-build uses the downloaded binary.
    ///
    /// # Arguments
    ///
    /// * `version` - The protoc version to download. Defaults to "31.1" if `None`.
    /// * `path` - The directory to download protoc into. Defaults to `OUT_DIR` if `None`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .fetch_protoc(None, None)?  // Use defaults
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "fetch-protoc")]
    pub fn fetch_protoc(mut self, version: Option<&str>, path: Option<&Path>) -> Result<Self> {
        let version = version.unwrap_or("31.1");
        let out_dir = match path {
            Some(p) => p.to_path_buf(),
            None => {
                let dir = std::env::var("OUT_DIR")
                    .map_err(|e| std::io::Error::other(format!("OUT_DIR not set: {e}")))?;
                PathBuf::from(dir)
            }
        };

        let protoc_path = protoc_fetcher::protoc(version, &out_dir)
            .map_err(|e| std::io::Error::other(format!("failed to fetch protoc: {e}")))?;

        self.protoc_path = Some(protoc_path);
        Ok(self)
    }

    /// Customize the prost builder with a configuration closure.
    ///
    /// The closure receives a mutable reference to `prost_build::Config` and is applied
    /// before the required internal configuration. Internal settings (like file descriptor
    /// set path) will be applied after and take precedence.
    ///
    /// Use this to add type attributes or other prost configuration.
    ///
    /// If you map protobuf packages to external crates with `extern_path`, and
    /// also generate pbjson serde code, configure matching pbjson extern mappings
    /// via [`with_pbjson_config`](Self::with_pbjson_config).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_prost_config(|config| {
    ///             config.type_attribute(".", "#[derive(Hash)]");
    ///         })
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn with_prost_config<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut prost_build::Config) + 'static,
    {
        self.prost_config = Some(Box::new(f));
        self
    }

    /// Customize the pbjson builder with a configuration closure.
    ///
    /// The closure receives a mutable reference to `pbjson_build::Builder` and is
    /// applied before descriptor registration and code generation.
    ///
    /// Use this when pbjson needs explicit extern mappings that mirror prost's
    /// `extern_path` behavior, for example:
    /// `builder.extern_path(".google.protobuf", "::pbjson_types");`
    pub fn with_pbjson_config<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut pbjson_build::Builder) + 'static,
    {
        self.pbjson_config = Some(Box::new(f));
        self
    }

    /// Set the output directory for generated code.
    ///
    /// By default, generated code is written to `OUT_DIR` (set by Cargo during build).
    /// Use this method to specify a custom output directory instead.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .out_dir("src/generated")
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn out_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.out_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Generate a single include file that provides a nested `pub mod` tree
    /// for all compiled protobuf packages.
    ///
    /// Instead of manually writing `mod pb { include!(...) }` boilerplate for each
    /// package, this generates a single file that you can include with:
    ///
    /// ```rust,ignore
    /// include!(concat!(env!("OUT_DIR"), "/protos.rs"));
    /// ```
    ///
    /// For dotted package names like `buf.validate`, this generates properly nested
    /// modules:
    ///
    /// ```rust,ignore
    /// pub mod buf {
    ///     pub mod validate {
    ///         include!(concat!(env!("OUT_DIR"), "/buf.validate.rs"));
    ///     }
    /// }
    /// ```
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .include_file("protos.rs")
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn include_file(mut self, path: impl AsRef<Path>) -> Self {
        self.include_file = Some(path.as_ref().to_path_buf());
        self
    }

    /// Register an extern module re-export for the include file.
    ///
    /// When using `extern_path` in prost config to map a protobuf package to an
    /// external crate (e.g. `config.extern_path(".google.protobuf", "::pbjson_types")`),
    /// prost-generated code may still reference types through relative module paths
    /// (e.g. `super::super::google::protobuf::...`). This method creates a shim module
    /// in the include file that re-exports the external crate's types, allowing those
    /// relative paths to resolve.
    ///
    /// # Arguments
    ///
    /// * `proto_path` - The dotted protobuf package name (e.g. `"google.protobuf"`)
    /// * `rust_path` - The Rust crate path to re-export (e.g. `"::pbjson_types"`)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_prost_config(|config| {
    ///             config.compile_well_known_types();
    ///             config.extern_path(".google.protobuf", "::pbjson_types");
    ///         })
    ///         .include_file("protos.rs")
    ///         .extern_module("google.protobuf", "::pbjson_types")
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn extern_module(
        mut self,
        proto_path: impl Into<String>,
        rust_path: impl Into<String>,
    ) -> Self {
        self.extern_reexports.push((proto_path.into(), rust_path.into()));
        self
    }
}

// ============================================================================
// Methods available when TonicClient = Disabled (enable tonic client)
// ============================================================================

#[cfg(feature = "tonic-client")]
impl<S, C, T, CC> CompileBuilder<S, C, T, Disabled, CC> {
    /// Enable generating tonic gRPC client stubs.
    ///
    /// Generates client code using `tonic-prost-build`. The client code is appended
    /// to the same `{package}.rs` file alongside message types and other generated code.
    ///
    /// This can be used independently of `with_tonic()` (server stubs).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_tonic_client()  // Generate gRPC clients
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn with_tonic_client(self) -> CompileBuilder<S, C, T, Enabled, CC> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_reexports: self.extern_reexports,

            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            prost_config: self.prost_config,
            pbjson_config: self.pbjson_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Methods available when TonicClient = Enabled (configure tonic client)
// ============================================================================

#[cfg(feature = "tonic-client")]
impl<S, C, T, CC> CompileBuilder<S, C, T, Enabled, CC> {
    /// Customize the tonic prost builder for client generation.
    ///
    /// The closure is applied before internal configuration. Internal settings
    /// (like `build_client(true)`, `build_server(false)`, `compile_well_known_types(false)`,
    /// `out_dir`, and `extern_path` mappings) will be applied after and take precedence.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_tonic_client()
    ///         .with_tonic_client_config(|builder| {
    ///             builder.build_transport(false)  // Disable transport feature
    ///         })
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn with_tonic_client_config<F>(mut self, f: F) -> Self
    where
        F: Fn(tonic_prost_build::Builder) -> tonic_prost_build::Builder + 'static,
    {
        self.tonic_client_config = Some(Box::new(f));
        self
    }
}

// ============================================================================
// Methods available when ConnectClient = Disabled (enable connect client)
// ============================================================================

impl<S, C, T, TC> CompileBuilder<S, C, T, TC, Disabled> {
    /// Enable generating typed Connect RPC client code.
    ///
    /// Generates client structs with typed methods for each RPC procedure.
    /// The generated client wraps [`ConnectClient`](connectrpc_axum_client::ConnectClient)
    /// and provides a more ergonomic API.
    ///
    /// This can be used independently of server code. Use `no_connect_server()` first
    /// if you only want client code without server code.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_connect_client()  // Generate typed Connect clients
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Generated Code
    ///
    /// For a service like:
    /// ```protobuf
    /// service HelloWorldService {
    ///   rpc SayHello(HelloRequest) returns (HelloResponse);
    /// }
    /// ```
    ///
    /// This generates:
    /// - `HELLO_WORLD_SERVICE_SERVICE_NAME` constant
    /// - `hello_world_service_procedures` module with procedure path constants
    /// - `HelloWorldServiceClient` struct with typed `say_hello()` method
    /// - `HelloWorldServiceClientBuilder` for configuration
    pub fn with_connect_client(self) -> CompileBuilder<S, C, T, TC, Enabled> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_reexports: self.extern_reexports,

            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            prost_config: self.prost_config,
            pbjson_config: self.pbjson_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Compile method - only available when Source = WithSource
// ============================================================================

impl<C: BuildMarker, T: BuildMarker, TC: BuildMarker, CC: BuildMarker> CompileBuilder<WithSource, C, T, TC, CC> {
    /// Execute code generation for the proto source.
    ///
    /// When `include_file` is set, a module tree file is generated after
    /// compilation by scanning the output directory.
    pub fn compile(&self) -> Result<()> {
        self.compile_source(&self.source.0)?;

        if let Some(ref include_path) = self.include_file {
            let out_dir = match &self.out_dir {
                Some(dir) => dir.display().to_string(),
                None => std::env::var("OUT_DIR")
                    .map_err(|e| std::io::Error::other(format!("OUT_DIR not set: {e}")))?,
            };
            let file_name = include_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Invalid include file path: {}", include_path.display()),
                    )
                })?;
            include_file::generate(
                file_name,
                &out_dir,
                &self.extern_reexports,
                self.out_dir.is_none(),
            )?;
        }

        Ok(())
    }

    fn compile_source(&self, source: &ProtoSource) -> Result<()> {
        use std::fs;

        let generate_handlers = C::VALUE;
        let grpc = T::VALUE;
        #[cfg(feature = "tonic-client")]
        let grpc_client = TC::VALUE;
        let connect_client = CC::VALUE;
        let out_dir = match &self.out_dir {
            Some(dir) => dir.display().to_string(),
            None => std::env::var("OUT_DIR")
                .map_err(|e| std::io::Error::other(format!("OUT_DIR not set: {e}")))?,
        };
        let descriptor_path = format!("{}/descriptor.bin", out_dir);

        // Resolve proto files and includes from the source
        let (proto_files, includes) = Self::resolve_source(source)?;

        // -------- Pass 1: prost + connect (conditionally) --------
        let mut config = prost_build::Config::default();

        // Set custom output directory if specified
        if self.out_dir.is_some() {
            config.out_dir(&out_dir);
        }

        // Apply user's prost configuration
        if let Some(ref config_fn) = self.prost_config {
            config_fn(&mut config);
        }

        // Set protoc executable if fetched (internal config takes precedence)
        #[cfg(feature = "fetch-protoc")]
        if let Some(ref protoc) = self.protoc_path {
            config.protoc_executable(protoc);
        }

        // Always generate descriptor set for pbjson-build (internal config takes precedence)
        config.file_descriptor_set_path(&descriptor_path);

        // Generate connect (and tonic-compatible wrapper builders if requested) in first pass
        if generate_handlers || connect_client {
            let service_generator = AxumConnectServiceGenerator::new()
                .with_connect_server(generate_handlers)
                .with_tonic(grpc)
                .with_connect_client(connect_client);
            config.service_generator(Box::new(service_generator));
        }

        let include_refs: Vec<&Path> = includes.iter().map(|p| p.as_path()).collect();
        config.compile_protos(&proto_files, &include_refs)?;

        // -------- Pass 1.5: pbjson serde implementations (always) --------
        Self::generate_pbjson(&out_dir, &descriptor_path, self.pbjson_config.as_ref())?;

        // -------- Pass 2: tonic server-only (feature + user requested) --------
        #[cfg(feature = "tonic")]
        if grpc {
            Self::generate_tonic_server(
                &out_dir,
                &descriptor_path,
                &proto_files,
                &includes,
                self.tonic_config.as_ref(),
            )?;
        }

        // -------- Pass 3: tonic client (feature + user requested) --------
        #[cfg(feature = "tonic-client")]
        if grpc_client {
            Self::generate_tonic_client(
                &out_dir,
                &descriptor_path,
                &proto_files,
                &includes,
                self.tonic_client_config.as_ref(),
            )?;
        }

        // Clean up descriptor file after all passes complete
        let _ = fs::remove_file(&descriptor_path);

        Ok(())
    }

    fn resolve_source(source: &ProtoSource) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        match source {
            ProtoSource::Directory(dir) => {
                let mut protos = Vec::new();
                discover_proto_files(dir, &mut protos)?;
                if protos.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("No .proto files found in directory: {}", dir.display()),
                    ));
                }
                Ok((protos, vec![dir.clone()]))
            }
            ProtoSource::Files { protos, includes } => {
                if protos.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "No proto files specified",
                    ));
                }
                Ok((protos.clone(), includes.clone()))
            }
        }
    }

    fn generate_pbjson(
        out_dir: &str,
        descriptor_path: &str,
        pbjson_config: Option<&Box<dyn Fn(&mut pbjson_build::Builder)>>,
    ) -> Result<()> {
        use std::fs;

        let descriptor_bytes = fs::read(descriptor_path)
            .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;

        let mut pbjson_builder = pbjson_build::Builder::new();
        pbjson_builder.out_dir(out_dir);
        if let Some(config_fn) = pbjson_config {
            config_fn(&mut pbjson_builder);
        }
        pbjson_builder
            .register_descriptors(&descriptor_bytes)
            .map_err(|e| std::io::Error::other(format!("register descriptors: {e}")))?
            .build(&["."]) // Generate for all packages
            .map_err(|e| std::io::Error::other(format!("pbjson build: {e}")))?;

        // Append pbjson serde implementations to main generated files
        // pbjson-build generates {package}.serde.rs files that need to be included
        for entry in fs::read_dir(out_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                && file_name.ends_with(".serde.rs")
            {
                // Get the base name (e.g., "hello" from "hello.serde.rs")
                let base_name = file_name.strip_suffix(".serde.rs").unwrap();
                let main_file = format!("{}/{}.rs", out_dir, base_name);

                if std::path::Path::new(&main_file).exists() {
                    // Append serde implementations to the main file
                    let mut content = fs::read_to_string(&main_file)?;
                    content.push_str("\n// --- pbjson serde implementations ---\n");
                    content.push_str(&fs::read_to_string(&path)?);
                    fs::write(&main_file, content)?;

                    // Remove the separate .serde.rs file
                    let _ = fs::remove_file(&path);
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "tonic")]
    fn generate_tonic_server(
        out_dir: &str,
        descriptor_path: &str,
        proto_files: &[PathBuf],
        includes: &[PathBuf],
        tonic_config: Option<&Box<dyn Fn(tonic_prost_build::Builder) -> tonic_prost_build::Builder>>,
    ) -> Result<()> {
        use prost::Message;
        use std::fs;

        let bytes = fs::read(descriptor_path)
            .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;
        let fds = prost_types::FileDescriptorSet::decode(bytes.as_slice())
            .map_err(|e| std::io::Error::other(format!("decode descriptor: {e}")))?;

        let type_refs = collect_type_refs(&fds);

        // Generate tonic server stubs referencing existing types
        let temp_out_dir = format!("{}/tonic_server", out_dir);
        fs::create_dir_all(&temp_out_dir)?;
        let mut builder = tonic_prost_build::configure();

        // Apply user's tonic configuration
        if let Some(config_fn) = tonic_config {
            builder = config_fn(builder);
        }

        // Apply internal config (takes precedence)
        builder = builder
            .build_client(false)
            .build_server(true)
            .compile_well_known_types(false)
            .out_dir(&temp_out_dir);

        // Add extern_path mappings for generated types
        for tr in &type_refs {
            builder = builder.extern_path(&tr.full, &tr.rust);
        }
        let proto_paths: Vec<&str> = proto_files.iter().map(|p| p.to_str().unwrap()).collect();
        let include_strs: Vec<&str> = includes.iter().map(|p| p.to_str().unwrap()).collect();
        builder.compile_protos(&proto_paths, &include_strs)?;

        // Append server code to first-pass files
        for entry in fs::read_dir(&temp_out_dir)? {
            let entry = entry?;
            let tonic_file = entry.path();

            // Only process .rs files
            if tonic_file.extension().and_then(|s| s.to_str()) == Some("rs") {
                let filename = tonic_file.file_name().unwrap().to_str().unwrap();
                let first_pass_file = format!("{}/{}", out_dir, filename);

                // Skip if no matching first-pass file (warn instead of error)
                if !std::path::Path::new(&first_pass_file).exists() {
                    println!(
                        "cargo:warning=Skipping tonic server file '{}': no matching first-pass file. \
                         This may indicate mismatched package declarations between prost-build and tonic-build.",
                        filename
                    );
                    continue;
                }

                // Append tonic server code to first-pass file
                let mut content = fs::read_to_string(&first_pass_file)?;
                content.push_str(
                    "\n// --- Tonic gRPC server stubs (extern_path reused messages) ---\n",
                );
                content.push_str(&fs::read_to_string(&tonic_file)?);
                fs::write(&first_pass_file, content)?;
            }
        }

        // Clean up temporary tonic artifacts
        let _ = fs::remove_dir_all(&temp_out_dir);

        Ok(())
    }

    #[cfg(feature = "tonic-client")]
    fn generate_tonic_client(
        out_dir: &str,
        descriptor_path: &str,
        proto_files: &[PathBuf],
        includes: &[PathBuf],
        tonic_client_config: Option<&Box<dyn Fn(tonic_prost_build::Builder) -> tonic_prost_build::Builder>>,
    ) -> Result<()> {
        use prost::Message;
        use std::fs;

        let bytes = fs::read(descriptor_path)
            .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;
        let fds = prost_types::FileDescriptorSet::decode(bytes.as_slice())
            .map_err(|e| std::io::Error::other(format!("decode descriptor: {e}")))?;

        let type_refs = collect_type_refs(&fds);

        // Generate tonic client stubs referencing existing types
        let temp_out_dir = format!("{}/tonic_client", out_dir);
        fs::create_dir_all(&temp_out_dir)?;
        let mut builder = tonic_prost_build::configure();

        // Apply user's tonic client configuration
        if let Some(config_fn) = tonic_client_config {
            builder = config_fn(builder);
        }

        // Apply internal config (takes precedence)
        builder = builder
            .build_client(true)
            .build_server(false)
            .compile_well_known_types(false)
            .out_dir(&temp_out_dir);

        // Add extern_path mappings for generated types
        for tr in &type_refs {
            builder = builder.extern_path(&tr.full, &tr.rust);
        }
        let proto_paths: Vec<&str> = proto_files.iter().map(|p| p.to_str().unwrap()).collect();
        let include_strs: Vec<&str> = includes.iter().map(|p| p.to_str().unwrap()).collect();
        builder.compile_protos(&proto_paths, &include_strs)?;

        // Append client code to first-pass files
        for entry in fs::read_dir(&temp_out_dir)? {
            let entry = entry?;
            let tonic_file = entry.path();

            // Only process .rs files
            if tonic_file.extension().and_then(|s| s.to_str()) == Some("rs") {
                let filename = tonic_file.file_name().unwrap().to_str().unwrap();
                let first_pass_file = format!("{}/{}", out_dir, filename);

                // Skip if no matching first-pass file (warn instead of error)
                if !std::path::Path::new(&first_pass_file).exists() {
                    println!(
                        "cargo:warning=Skipping tonic client file '{}': no matching first-pass file. \
                         This may indicate mismatched package declarations between prost-build and tonic-build.",
                        filename
                    );
                    continue;
                }

                // Append tonic client code to first-pass file
                let mut content = fs::read_to_string(&first_pass_file)?;
                content.push_str(
                    "\n// --- Tonic gRPC client stubs (extern_path reused messages) ---\n",
                );
                content.push_str(&fs::read_to_string(&tonic_file)?);
                fs::write(&first_pass_file, content)?;
            }
        }

        // Clean up temporary tonic client artifacts
        let _ = fs::remove_dir_all(&temp_out_dir);

        Ok(())
    }
}
#[cfg(any(feature = "tonic", feature = "tonic-client"))]
#[derive(Debug)]
struct TypeRef {
    full: String,
    rust: String,
}

#[cfg(any(feature = "tonic", feature = "tonic-client"))]
fn collect_type_refs(fds: &prost_types::FileDescriptorSet) -> Vec<TypeRef> {
    let mut out = Vec::new();
    for file in &fds.file {
        let pkg = file.package.clone().unwrap_or_default();
        // Process all files, including those without package declarations
        for msg in &file.message_type {
            recurse_message(&pkg, msg, &[], &mut out);
        }
        for en in &file.enum_type {
            recurse_enum(&pkg, en, &[], &mut out);
        }
    }
    out
}

#[cfg(any(feature = "tonic", feature = "tonic-client"))]
fn recurse_message(
    pkg: &str,
    msg: &prost_types::DescriptorProto,
    parents: &[String],
    out: &mut Vec<TypeRef>,
) {
    let name = msg.name.as_deref().unwrap_or("").to_string();
    if !name.is_empty() {
        // Generate protobuf fully-qualified name, handling empty packages
        let full_proto = if pkg.is_empty() {
            // No package: .TypeName or .Parent.TypeName
            format!(
                ".{}{}",
                if parents.is_empty() {
                    String::new()
                } else {
                    format!("{}.", parents.join("."))
                },
                name
            )
        } else {
            // Has package: .pkg.TypeName or .pkg.Parent.TypeName
            format!(
                ".{}.{}{}",
                pkg,
                if parents.is_empty() {
                    String::new()
                } else {
                    format!("{}.", parents.join("."))
                },
                name
            )
        };
        // Prost flattens nested types by prefixing parent names with underscores; we mimic by joining with '_' for nested mapping.
        let rust_ident = if parents.is_empty() {
            name.clone()
        } else {
            format!("{}_{}", parents.join("_"), name)
        };
        // Don't use crate:: or super:: prefix because tonic will add `super::` when generating
        // code inside the service module. Since the types are at the file root and the trait
        // is inside a nested module (e.g., hello_world_service_server), tonic will correctly
        // reference them as `super::TypeName` from inside the module.
        out.push(TypeRef {
            full: full_proto,
            rust: rust_ident,
        });
    }
    let mut new_parents = parents.to_vec();
    if !name.is_empty() {
        new_parents.push(name.clone());
    }
    for nested in &msg.nested_type {
        recurse_message(pkg, nested, &new_parents, out);
    }
    for en in &msg.enum_type {
        recurse_enum(pkg, en, &new_parents, out);
    }
}

#[cfg(any(feature = "tonic", feature = "tonic-client"))]
fn recurse_enum(
    pkg: &str,
    en: &prost_types::EnumDescriptorProto,
    parents: &[String],
    out: &mut Vec<TypeRef>,
) {
    let name = en.name.as_deref().unwrap_or("").to_string();
    if !name.is_empty() {
        // Generate protobuf fully-qualified name, handling empty packages
        let full_proto = if pkg.is_empty() {
            // No package: .TypeName or .Parent.TypeName
            format!(
                ".{}{}",
                if parents.is_empty() {
                    String::new()
                } else {
                    format!("{}.", parents.join("."))
                },
                name
            )
        } else {
            // Has package: .pkg.TypeName or .pkg.Parent.TypeName
            format!(
                ".{}.{}{}",
                pkg,
                if parents.is_empty() {
                    String::new()
                } else {
                    format!("{}.", parents.join("."))
                },
                name
            )
        };
        let rust_ident = if parents.is_empty() {
            name.clone()
        } else {
            format!("{}_{}", parents.join("_"), name)
        };
        // Don't use crate:: or super:: prefix because tonic will add `super::` when generating
        // code inside the service module. Since the types are at the file root and the trait
        // is inside a nested module (e.g., hello_world_service_server), tonic will correctly
        // reference them as `super::TypeName` from inside the module.
        out.push(TypeRef {
            full: full_proto,
            rust: rust_ident,
        });
    }
}

/// Create a builder without a proto source.
///
/// Use [`compile_dir()`](CompileBuilder::compile_dir) or
/// [`compile_protos()`](CompileBuilder::compile_protos) to set the source
/// before calling [`compile()`](CompileBuilder::compile).
///
/// # Example
///
/// ```rust,ignore
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     connectrpc_axum_build::builder()
///         .include_file("protos.rs")
///         .compile_dir("proto")
///         .compile()?;
///     Ok(())
/// }
/// ```
pub fn builder() -> CompileBuilder {
    CompileBuilder {
        source: NoSource,
        out_dir: None,
        include_file: None,
        extern_reexports: Vec::new(),

        #[cfg(feature = "fetch-protoc")]
        protoc_path: None,
        prost_config: None,
        pbjson_config: None,
        #[cfg(feature = "tonic")]
        tonic_config: None,
        #[cfg(feature = "tonic-client")]
        tonic_client_config: None,
        _marker: PhantomData,
    }
}

/// Convenience function that auto-discovers all .proto files in the directory
/// and returns a builder ready to compile.
///
/// This provides the best developer experience by only requiring the proto directory path.
///
/// # Examples
///
/// ```rust,no_run
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     connectrpc_axum_build::compile_dir("proto").compile()?;
///     Ok(())
/// }
/// ```
///
/// With custom configuration:
/// ```rust,no_run
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     connectrpc_axum_build::compile_dir("proto")
///         .with_prost_config(|config| {
///             config.type_attribute(".", "#[derive(Debug)]");
///         })
///         .compile()?;
///     Ok(())
/// }
/// ```
pub fn compile_dir(dir: impl AsRef<Path>) -> CompileBuilder<WithSource> {
    CompileBuilder {
        source: WithSource(ProtoSource::Directory(dir.as_ref().to_path_buf())),
        out_dir: None,
        include_file: None,
        extern_reexports: Vec::new(),

        #[cfg(feature = "fetch-protoc")]
        protoc_path: None,
        prost_config: None,
        pbjson_config: None,
        #[cfg(feature = "tonic")]
        tonic_config: None,
        #[cfg(feature = "tonic-client")]
        tonic_client_config: None,
        _marker: PhantomData,
    }
}

/// Convenience function that compiles specific proto files with explicit include directories.
///
/// # Arguments
///
/// * `protos` - Proto files to compile
/// * `includes` - Directories to search for imports
///
/// # Example
///
/// ```rust,ignore
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     connectrpc_axum_build::compile_protos(
///         &["proto/service.proto", "proto/messages.proto"],
///         &["proto", "third_party"],
///     ).compile()?;
///     Ok(())
/// }
/// ```
pub fn compile_protos<P: AsRef<Path>>(protos: &[P], includes: &[P]) -> CompileBuilder<WithSource> {
    CompileBuilder {
        source: WithSource(ProtoSource::Files {
            protos: protos.iter().map(|p| p.as_ref().to_path_buf()).collect(),
            includes: includes.iter().map(|p| p.as_ref().to_path_buf()).collect(),
        }),
        out_dir: None,
        include_file: None,
        extern_reexports: Vec::new(),

        #[cfg(feature = "fetch-protoc")]
        protoc_path: None,
        prost_config: None,
        pbjson_config: None,
        #[cfg(feature = "tonic")]
        tonic_config: None,
        #[cfg(feature = "tonic-client")]
        tonic_client_config: None,
        _marker: PhantomData,
    }
}

fn discover_proto_files(dir: &Path, proto_files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Directory not found: {}", dir.display()),
        ));
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("proto") {
            proto_files.push(path);
        } else if path.is_dir() {
            // Recursively search subdirectories
            discover_proto_files(&path, proto_files)?;
        }
    }

    Ok(())
}
