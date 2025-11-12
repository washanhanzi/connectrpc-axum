use r#gen::AxumConnectServiceGenerator;
use std::io::Result;
use std::path::{Path, PathBuf};

mod r#gen;

// Re-export build-time deps so consumers don't need to depend on them directly
pub use prost_build;
#[cfg(feature = "tonic")]
pub use tonic_prost_build;

/// Builder for compiling proto files with optional configuration.
pub struct CompileBuilder {
    includes_dir: PathBuf,
    config: Option<prost_build::Config>,
    grpc: bool,
}

impl CompileBuilder {
    /// Create a new builder for the given includes directory.
    pub fn new(includes_dir: impl AsRef<Path>) -> Self {
        Self {
            includes_dir: includes_dir.as_ref().to_path_buf(),
            config: None,
            grpc: false,
        }
    }

    /// Provide a custom prost_build::Config (still augmented with serde + default attributes).
    pub fn with_config(mut self, config: prost_build::Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Enable generating tonic gRPC server stubs (second pass) + tonic-compatible helpers in first pass.
    #[cfg(feature = "tonic")]
    pub fn with_tonic(mut self) -> Self {
        self.grpc = true;
        self
    }

    /// Execute code generation.
    pub fn compile(self) -> Result<()> {
        let out_dir = std::env::var("OUT_DIR")
            .map_err(|e| std::io::Error::other(format!("OUT_DIR not set: {e}")))?;
        let descriptor_path = format!("{}/descriptor.bin", out_dir);

        // -------- Pass 1: prost + connect (always) --------
        let mut config = self.config.unwrap_or_default();

        // Always generate descriptor set for pbjson-build
        config.file_descriptor_set_path(&descriptor_path);

        // Configure well-known types for proper pbjson integration
        config.compile_well_known_types();
        config.extern_path(".google.protobuf", "::pbjson_types");

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
        let service_generator = AxumConnectServiceGenerator::with_tonic(self.grpc);
        config.service_generator(Box::new(service_generator));
        config.compile_protos(&proto_files, &[&self.includes_dir])?;

        // -------- Pass 1.5: pbjson serde implementations (always) --------
        // Use pbjson-build to generate proper serde implementations that handle oneof correctly
        use std::fs;
        let descriptor_bytes = fs::read(&descriptor_path)
            .map_err(|e| std::io::Error::other(format!("read descriptor: {e}")))?;

        pbjson_build::Builder::new()
            .register_descriptors(&descriptor_bytes)
            .map_err(|e| std::io::Error::other(format!("register descriptors: {e}")))?
            .preserve_proto_field_names() // Keep original proto field names
            .build(&["."])  // Generate for all packages
            .map_err(|e| std::io::Error::other(format!("pbjson build: {e}")))?;

        // Append pbjson serde implementations to main generated files
        // pbjson-build generates {package}.serde.rs files that need to be included
        for entry in fs::read_dir(&out_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".serde.rs") {
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
        }

        // -------- Pass 2: tonic server-only (feature + user requested) --------
        #[cfg(feature = "tonic")]
        if self.grpc {
            use prost::Message; // for descriptor decode
            use std::fs;

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
            builder = builder
                .build_client(false)
                .build_server(true)
                .out_dir(&temp_out_dir);
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

                    // Append tonic server code to first-pass file
                    let mut content = fs::read_to_string(&first_pass_file).unwrap_or_default();
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

        // Clean up descriptor file after all passes complete
        let _ = std::fs::remove_file(&descriptor_path);

        Ok(())
    }
}
#[cfg(feature = "tonic")]
#[derive(Debug)]
struct TypeRef {
    full: String,
    rust: String,
}

#[cfg(feature = "tonic")]
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

#[cfg(feature = "tonic")]
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
        out.push(TypeRef {
            full: full_proto,
            rust: format!("crate::{}", rust_ident),
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

#[cfg(feature = "tonic")]
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
        out.push(TypeRef {
            full: full_proto,
            rust: format!("crate::{}", rust_ident),
        });
    }
}
// (Note) Previous text-stripping approach removed; now we rely on extern_path mappings in a second pass
// to avoid regenerating protobuf message definitions when producing tonic server stubs.

/// Convenience function that auto-discovers all .proto files in the includes directory
/// and compiles them with a default or custom configuration.
///
/// This provides the best developer experience by only requiring the includes path.
/// Use `.with_config()` if you need custom configuration.
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
/// use prost_build::Config;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let mut config = Config::new();
///     config.type_attribute(".", "#[derive(Debug)]");
///
///     connectrpc_axum_build::compile_dir("proto")
///         .with_config(config)
///         .compile()?;
///     Ok(())
/// }
/// ```
///
/// With gRPC support:
/// ```rust,no_run
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     connectrpc_axum_build::compile_dir("proto")
///         .with_tonic()  // Enable Tonic gRPC code generation
///         .compile()?;
///     Ok(())
/// }
/// ```
pub fn compile_dir(includes_dir: impl AsRef<Path>) -> CompileBuilder {
    CompileBuilder::new(includes_dir)
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
