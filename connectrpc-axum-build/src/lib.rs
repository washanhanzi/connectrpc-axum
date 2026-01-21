use r#gen::AxumConnectServiceGenerator;
use std::io::Result;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

/// Code generation module for service builders.
mod r#gen;

// ============================================================================
// Type-state marker types for phantom data
// ============================================================================

/// Marker indicating a feature is enabled.
pub struct Enabled;

/// Marker indicating a feature is disabled.
pub struct Disabled;

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
/// - `Connect`: Whether to generate Connect service handlers
/// - `Tonic`: Whether to generate Tonic gRPC server stubs (requires `tonic` feature)
/// - `TonicClient`: Whether to generate Tonic gRPC client stubs (requires `tonic-client` feature)
///
/// Default state is `CompileBuilder<Enabled, Disabled, Disabled>` (Connect handlers only).
pub struct CompileBuilder<Connect = Enabled, Tonic = Disabled, TonicClient = Disabled> {
    includes_dir: PathBuf,
    prost_config: Option<Box<dyn FnOnce(&mut prost_build::Config)>>,
    #[cfg(feature = "tonic")]
    tonic_config: Option<Box<dyn FnOnce(tonic_prost_build::Builder) -> tonic_prost_build::Builder>>,
    #[cfg(feature = "tonic-client")]
    tonic_client_config:
        Option<Box<dyn FnOnce(tonic_prost_build::Builder) -> tonic_prost_build::Builder>>,
    _marker: PhantomData<(Connect, Tonic, TonicClient)>,
}

// ============================================================================
// Methods available when Connect = Enabled
// ============================================================================

impl<T, TC> CompileBuilder<Enabled, T, TC> {
    /// Skip generating Connect service handlers.
    ///
    /// When called, only message types and serde implementations are generated.
    /// No Connect service builders (e.g., `HelloWorldServiceBuilder`) will be created.
    ///
    /// **Note:** After calling this, `with_tonic()` is no longer available since
    /// tonic server stubs depend on handler builders.
    ///
    /// Use this when you only need protobuf message types with JSON serialization support.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .no_handlers()  // Only generate message types + serde
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn no_handlers(self) -> CompileBuilder<Disabled, Disabled, TC> {
        CompileBuilder {
            includes_dir: self.includes_dir,
            prost_config: self.prost_config,
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
impl<TC> CompileBuilder<Enabled, Disabled, TC> {
    /// Enable generating tonic gRPC server stubs (second pass) + tonic-compatible helpers in first pass.
    ///
    /// **Note:** After calling this, `no_handlers()` is no longer available since
    /// tonic server stubs depend on handler builders.
    pub fn with_tonic(self) -> CompileBuilder<Enabled, Enabled, TC> {
        CompileBuilder {
            includes_dir: self.includes_dir,
            prost_config: self.prost_config,
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
impl<C, TC> CompileBuilder<C, Enabled, TC> {
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
    ///             config.extern_path(".google.protobuf", "::pbjson_types");
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
        F: FnOnce(tonic_prost_build::Builder) -> tonic_prost_build::Builder + 'static,
    {
        self.tonic_config = Some(Box::new(f));
        self
    }
}

// ============================================================================
// Methods available on all builder states
// ============================================================================

impl<C, T, TC> CompileBuilder<C, T, TC> {
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
    pub fn fetch_protoc(self, version: Option<&str>, path: Option<&Path>) -> Result<Self> {
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

        // SAFETY: This is called from build.rs which runs single-threaded before compilation.
        // No other threads exist that could be reading environment variables concurrently.
        unsafe {
            std::env::set_var("PROTOC", protoc_path);
        }

        Ok(self)
    }

    /// Customize the prost builder with a configuration closure.
    ///
    /// The closure receives a mutable reference to `prost_build::Config` and is applied
    /// before the required internal configuration. Internal settings (like file descriptor
    /// set path) will be applied after and take precedence.
    ///
    /// Use this to add type attributes, extern paths, or other prost configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     connectrpc_axum_build::compile_dir("proto")
    ///         .with_prost_config(|config| {
    ///             config.extern_path(".google.protobuf", "::pbjson_types");
    ///         })
    ///         .compile()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn with_prost_config<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut prost_build::Config) + 'static,
    {
        self.prost_config = Some(Box::new(f));
        self
    }
}

// ============================================================================
// Methods available when TonicClient = Disabled (enable tonic client)
// ============================================================================

#[cfg(feature = "tonic-client")]
impl<C, T> CompileBuilder<C, T, Disabled> {
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
    pub fn with_tonic_client(self) -> CompileBuilder<C, T, Enabled> {
        CompileBuilder {
            includes_dir: self.includes_dir,
            prost_config: self.prost_config,
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
impl<C, T> CompileBuilder<C, T, Enabled> {
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
        F: FnOnce(tonic_prost_build::Builder) -> tonic_prost_build::Builder + 'static,
    {
        self.tonic_client_config = Some(Box::new(f));
        self
    }
}

// ============================================================================
// Compile method - available on all states with BoolMarker bounds
// ============================================================================

impl<C: BuildMarker, T: BuildMarker, TC: BuildMarker> CompileBuilder<C, T, TC> {
    /// Execute code generation.
    pub fn compile(self) -> Result<()> {
        let generate_handlers = C::VALUE;
        let grpc = T::VALUE;
        #[cfg(feature = "tonic-client")]
        let grpc_client = TC::VALUE;
        let out_dir = std::env::var("OUT_DIR")
            .map_err(|e| std::io::Error::other(format!("OUT_DIR not set: {e}")))?;
        let descriptor_path = format!("{}/descriptor.bin", out_dir);

        // -------- Pass 1: prost + connect (conditionally) --------
        let mut config = prost_build::Config::default();

        // Apply user's prost configuration first
        if let Some(config_fn) = self.prost_config {
            config_fn(&mut config);
        }

        // Always generate descriptor set for pbjson-build (internal config takes precedence)
        config.file_descriptor_set_path(&descriptor_path);

        let mut proto_files = Vec::new();
        discover_proto_files(&self.includes_dir, &mut proto_files)?;
        if proto_files.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "No .proto files found in directory: {}",
                    self.includes_dir.display()
                ),
            ));
        }

        // Generate connect (and tonic-compatible wrapper builders if requested) in first pass
        if generate_handlers {
            let service_generator = AxumConnectServiceGenerator::with_tonic(grpc);
            config.service_generator(Box::new(service_generator));
        }
        config.compile_protos(&proto_files, &[&self.includes_dir])?;

        // -------- Pass 1.5: pbjson serde implementations (always) --------
        // Use pbjson-build to generate proper serde implementations that handle oneof correctly
        use std::fs;
        let descriptor_bytes = fs::read(&descriptor_path)
            .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;

        pbjson_build::Builder::new()
            .register_descriptors(&descriptor_bytes)
            .map_err(|e| std::io::Error::other(format!("register descriptors: {e}")))?
            .build(&["."]) // Generate for all packages
            .map_err(|e| std::io::Error::other(format!("pbjson build: {e}")))?;

        // Append pbjson serde implementations to main generated files
        // pbjson-build generates {package}.serde.rs files that need to be included
        for entry in fs::read_dir(&out_dir)? {
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

        // -------- Pass 2: tonic server-only (feature + user requested) --------
        #[cfg(feature = "tonic")]
        if grpc {
            use prost::Message; // for descriptor decode

            let out_dir = std::env::var("OUT_DIR").unwrap();
            let descriptor_path = format!("{}/descriptor.bin", out_dir);
            let bytes = fs::read(&descriptor_path)
                .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;
            let fds = prost_types::FileDescriptorSet::decode(bytes.as_slice())
                .map_err(|e| std::io::Error::other(format!("decode descriptor: {e}")))?;

            let type_refs = collect_type_refs(&fds);

            // Generate tonic server stubs referencing existing types
            let temp_out_dir = format!("{}/tonic_server", out_dir);
            fs::create_dir_all(&temp_out_dir)?;
            let mut builder = tonic_prost_build::configure();

            // Apply user's tonic configuration first
            if let Some(config_fn) = self.tonic_config {
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
            builder.compile_protos(
                &proto_paths,
                &[self.includes_dir.as_path().to_str().unwrap()],
            )?;

            // Append server code to first-pass files
            // Iterate generated files in tonic_server/ instead of proto_files
            // because prost-build generates one file per package, not per proto file
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

            // Clean up temporary tonic artifacts so include!(concat!(env!("OUT_DIR"), "/<file>.rs")) users don't see extras.
            let _ = fs::remove_dir_all(&temp_out_dir);
        }

        // -------- Pass 3: tonic client (feature + user requested) --------
        #[cfg(feature = "tonic-client")]
        if grpc_client {
            use prost::Message; // for descriptor decode

            let out_dir = std::env::var("OUT_DIR").unwrap();
            let descriptor_path = format!("{}/descriptor.bin", out_dir);
            let bytes = fs::read(&descriptor_path)
                .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;
            let fds = prost_types::FileDescriptorSet::decode(bytes.as_slice())
                .map_err(|e| std::io::Error::other(format!("decode descriptor: {e}")))?;

            let type_refs = collect_type_refs(&fds);

            // Generate tonic client stubs referencing existing types
            let temp_out_dir = format!("{}/tonic_client", out_dir);
            fs::create_dir_all(&temp_out_dir)?;
            let mut builder = tonic_prost_build::configure();

            // Apply user's tonic client configuration first
            if let Some(config_fn) = self.tonic_client_config {
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
            builder.compile_protos(
                &proto_paths,
                &[self.includes_dir.as_path().to_str().unwrap()],
            )?;

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
        }

        // Clean up descriptor file after all passes complete
        let _ = std::fs::remove_file(&descriptor_path);

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
// (Note) Previous text-stripping approach removed; now we rely on extern_path mappings in a second pass
// to avoid regenerating protobuf message definitions when producing tonic server stubs.

/// Convenience function that auto-discovers all .proto files in the includes directory
/// and compiles them with a default or custom configuration.
///
/// This provides the best developer experience by only requiring the includes path.
/// Use `.with_prost_config()` if you need custom configuration.
///
/// # Examples
///
/// Basic usage with default configuration:
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
///
/// With gRPC support (requires `tonic` feature):
/// ```rust,ignore
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     connectrpc_axum_build::compile_dir("proto")
///         .with_tonic()  // Enable Tonic gRPC code generation
///         .compile()?;
///     Ok(())
/// }
/// ```
pub fn compile_dir(includes_dir: impl AsRef<Path>) -> CompileBuilder {
    CompileBuilder {
        includes_dir: includes_dir.as_ref().to_path_buf(),
        prost_config: None,
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
