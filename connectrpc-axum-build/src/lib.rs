use convert_case::{Case, Casing};
use r#gen::AxumConnectServiceGenerator;
use include_file::IncludeEntry;
use model::{ProtoService, build_services, collect_file_messages};
use prost::Message as _;
use prost_types::FileDescriptorSet;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Error, ErrorKind, Result};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Command;

mod r#gen;
mod include_file;
mod model;

type BuffaConfigFn = dyn Fn(buffa_build::Config) -> buffa_build::Config;
#[cfg(any(feature = "tonic", feature = "tonic-client"))]
type TonicConfigFn = dyn Fn(tonic_buffa_build::Builder) -> tonic_buffa_build::Builder;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TonicRequestMode {
    #[default]
    Owned,
    View,
}

enum ProtoSource {
    Directory(PathBuf),
    Files {
        protos: Vec<PathBuf>,
        includes: Vec<PathBuf>,
    },
}

pub struct Enabled;
pub struct Disabled;
pub struct NoSource;
pub struct WithSource(ProtoSource);

pub trait BuildMarker {
    const VALUE: bool;
}

impl BuildMarker for Enabled {
    const VALUE: bool = true;
}

impl BuildMarker for Disabled {
    const VALUE: bool = false;
}

pub struct CompileBuilder<
    Source = NoSource,
    Connect = Enabled,
    Tonic = Disabled,
    TonicClient = Disabled,
    ConnectClient = Disabled,
> {
    source: Source,
    out_dir: Option<PathBuf>,
    include_file: Option<PathBuf>,
    extern_paths: Vec<(String, String)>,
    extern_reexports: Vec<(String, String)>,
    #[cfg(feature = "fetch-protoc")]
    protoc_path: Option<PathBuf>,
    buffa_config: Option<Box<BuffaConfigFn>>,
    #[cfg(feature = "tonic")]
    tonic_config: Option<Box<TonicConfigFn>>,
    tonic_request_mode: TonicRequestMode,
    #[cfg(feature = "tonic-client")]
    tonic_client_config: Option<Box<TonicConfigFn>>,
    _marker: PhantomData<(Connect, Tonic, TonicClient, ConnectClient)>,
}

struct GenerationContext {
    out_dir: PathBuf,
    descriptor_path: PathBuf,
    include_from_out_dir_env: bool,
    requested_descriptor_files: Vec<String>,
    requested_file_packages: HashMap<String, String>,
    requested_fds: FileDescriptorSet,
    services: Vec<ProtoService>,
    #[cfg(any(feature = "tonic", feature = "tonic-client"))]
    requested_service_packages: Vec<String>,
    shared_extern_paths: Vec<(String, String)>,
}

impl<C, T, TC, CC> CompileBuilder<NoSource, C, T, TC, CC> {
    pub fn compile_dir(self, dir: impl AsRef<Path>) -> CompileBuilder<WithSource, C, T, TC, CC> {
        CompileBuilder {
            source: WithSource(ProtoSource::Directory(dir.as_ref().to_path_buf())),
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_paths: self.extern_paths,
            extern_reexports: self.extern_reexports,
            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            buffa_config: self.buffa_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            tonic_request_mode: self.tonic_request_mode,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }

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
            extern_paths: self.extern_paths,
            extern_reexports: self.extern_reexports,
            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            buffa_config: self.buffa_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            tonic_request_mode: self.tonic_request_mode,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

impl<S, T, TC, CC> CompileBuilder<S, Enabled, T, TC, CC> {
    pub fn no_connect_server(self) -> CompileBuilder<S, Disabled, Disabled, TC, Disabled> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_paths: self.extern_paths,
            extern_reexports: self.extern_reexports,
            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            buffa_config: self.buffa_config,
            #[cfg(feature = "tonic")]
            tonic_config: None,
            tonic_request_mode: self.tonic_request_mode,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

#[cfg(feature = "tonic")]
impl<S, TC, CC> CompileBuilder<S, Enabled, Disabled, TC, CC> {
    pub fn with_tonic(self) -> CompileBuilder<S, Enabled, Enabled, TC, CC> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_paths: self.extern_paths,
            extern_reexports: self.extern_reexports,
            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            buffa_config: self.buffa_config,
            tonic_config: self.tonic_config,
            tonic_request_mode: self.tonic_request_mode,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

#[cfg(feature = "tonic")]
impl<S, C, TC, CC> CompileBuilder<S, C, Enabled, TC, CC> {
    pub fn with_tonic_prost_config<F>(mut self, f: F) -> Self
    where
        F: Fn(tonic_buffa_build::Builder) -> tonic_buffa_build::Builder + 'static,
    {
        self.tonic_config = Some(Box::new(f));
        self
    }

    pub fn with_tonic_request_mode(mut self, mode: TonicRequestMode) -> Self {
        self.tonic_request_mode = mode;
        self
    }
}

#[cfg(feature = "tonic-client")]
impl<S, C, T, CC> CompileBuilder<S, C, T, Disabled, CC> {
    pub fn with_tonic_client(self) -> CompileBuilder<S, C, T, Enabled, CC> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_paths: self.extern_paths,
            extern_reexports: self.extern_reexports,
            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            buffa_config: self.buffa_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            tonic_request_mode: self.tonic_request_mode,
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

#[cfg(feature = "tonic-client")]
impl<S, C, T, CC> CompileBuilder<S, C, T, Enabled, CC> {
    pub fn with_tonic_client_config<F>(mut self, f: F) -> Self
    where
        F: Fn(tonic_buffa_build::Builder) -> tonic_buffa_build::Builder + 'static,
    {
        self.tonic_client_config = Some(Box::new(f));
        self
    }
}

impl<S, C, T, TC> CompileBuilder<S, C, T, TC, Disabled> {
    pub fn with_connect_client(self) -> CompileBuilder<S, C, T, TC, Enabled> {
        CompileBuilder {
            source: self.source,
            out_dir: self.out_dir,
            include_file: self.include_file,
            extern_paths: self.extern_paths,
            extern_reexports: self.extern_reexports,
            #[cfg(feature = "fetch-protoc")]
            protoc_path: self.protoc_path,
            buffa_config: self.buffa_config,
            #[cfg(feature = "tonic")]
            tonic_config: self.tonic_config,
            tonic_request_mode: self.tonic_request_mode,
            #[cfg(feature = "tonic-client")]
            tonic_client_config: self.tonic_client_config,
            _marker: PhantomData,
        }
    }
}

impl<S, C, T, TC, CC> CompileBuilder<S, C, T, TC, CC> {
    #[cfg(feature = "fetch-protoc")]
    pub fn fetch_protoc(mut self, version: Option<&str>, path: Option<&Path>) -> Result<Self> {
        let version = version.unwrap_or("31.1");
        let out_dir = match path {
            Some(path) => path.to_path_buf(),
            None => PathBuf::from(out_dir_env()?),
        };

        let protoc_path = protoc_fetcher::protoc(version, &out_dir)
            .map_err(|e| Error::other(format!("failed to fetch protoc: {e}")))?;

        self.protoc_path = Some(protoc_path);
        Ok(self)
    }

    pub fn with_buffa_config<F>(mut self, f: F) -> Self
    where
        F: Fn(buffa_build::Config) -> buffa_build::Config + 'static,
    {
        self.buffa_config = Some(Box::new(f));
        self
    }

    pub fn out_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.out_dir = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn include_file(mut self, path: impl AsRef<Path>) -> Self {
        self.include_file = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn extern_path(
        mut self,
        proto_path: impl Into<String>,
        rust_path: impl Into<String>,
    ) -> Self {
        self.extern_paths
            .push((normalize_proto_path(proto_path.into()), rust_path.into()));
        self
    }

    pub fn extern_module(
        mut self,
        proto_path: impl Into<String>,
        rust_path: impl Into<String>,
    ) -> Self {
        self.extern_reexports.push((
            proto_path.into().trim_start_matches('.').to_string(),
            rust_path.into(),
        ));
        self
    }
}

impl<C: BuildMarker, T: BuildMarker, TC: BuildMarker, CC: BuildMarker>
    CompileBuilder<WithSource, C, T, TC, CC>
{
    pub fn compile(&self) -> Result<()> {
        self.compile_source(&self.source.0)
    }

    fn compile_source(&self, source: &ProtoSource) -> Result<()> {
        let generate_connect_server = C::VALUE;
        let generate_tonic_server = T::VALUE;
        #[cfg(feature = "tonic-client")]
        let generate_tonic_client = TC::VALUE;
        let generate_connect_client = CC::VALUE;

        let context = self.build_generation_context(source)?;
        let mut include_entries = Vec::new();

        self.generate_buffa_messages(&context, &mut include_entries)?;
        self.generate_view_glue(&context, &mut include_entries)?;

        if generate_connect_server || generate_connect_client {
            self.generate_connect_sidecars(
                &context,
                &context.services,
                generate_connect_server,
                generate_tonic_server,
                generate_connect_client,
                &mut include_entries,
            )?;
        }

        #[cfg(feature = "tonic")]
        if generate_tonic_server {
            self.generate_tonic_sidecars(
                &context,
                true,
                false,
                "tonic",
                self.tonic_config.as_ref(),
                &mut include_entries,
            )?;
        }

        #[cfg(feature = "tonic-client")]
        if generate_tonic_client {
            self.generate_tonic_sidecars(
                &context,
                false,
                true,
                "tonic_client",
                self.tonic_client_config.as_ref(),
                &mut include_entries,
            )?;
        }

        if let Some(include_path) = &self.include_file {
            let file_name = include_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        format!("invalid include file path: {}", include_path.display()),
                    )
                })?;

            include_file::generate(
                file_name,
                context.out_dir.to_str().ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        format!("invalid output directory: {}", context.out_dir.display()),
                    )
                })?,
                &include_entries,
                &self.extern_reexports,
                context.include_from_out_dir_env,
            )?;
        }

        let _ = std::fs::remove_file(&context.descriptor_path);

        Ok(())
    }

    fn build_generation_context(&self, source: &ProtoSource) -> Result<GenerationContext> {
        let out_dir = match &self.out_dir {
            Some(path) => path.clone(),
            None => PathBuf::from(out_dir_env()?),
        };
        std::fs::create_dir_all(&out_dir)?;

        let descriptor_path = out_dir.join("descriptor.bin");
        let (proto_files, includes) = Self::resolve_source(source)?;

        let requested_descriptor_files = normalize_requested_files(&proto_files, &includes)?;
        let requested_file_set: HashSet<_> = requested_descriptor_files.iter().cloned().collect();
        #[cfg(feature = "fetch-protoc")]
        let protoc_path = self.protoc_path.as_ref();
        #[cfg(not(feature = "fetch-protoc"))]
        let protoc_path = None;

        let full_fds = run_protoc_descriptor_set(&proto_files, &includes, protoc_path)?;

        std::fs::write(&descriptor_path, full_fds.encode_to_vec())?;

        let mut requested_file_packages = HashMap::new();

        for file in &full_fds.file {
            let Some(name) = file.name.clone() else {
                continue;
            };
            if !requested_file_set.contains(&name) {
                continue;
            }

            let package = file.package.clone().unwrap_or_default();
            requested_file_packages.insert(name, package.clone());
        }

        let requested_fds = prune_descriptor_set(&full_fds, &requested_file_set)?;
        let shared_extern_paths =
            effective_shared_extern_paths(&full_fds, &requested_file_set, &self.extern_paths);
        let services = build_services(&requested_fds, &requested_file_set, &shared_extern_paths)?;
        #[cfg(any(feature = "tonic", feature = "tonic-client"))]
        let requested_service_packages = services
            .iter()
            .map(|service| service.package.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        Ok(GenerationContext {
            out_dir,
            descriptor_path,
            include_from_out_dir_env: self.out_dir.is_none(),
            requested_descriptor_files,
            requested_file_packages,
            requested_fds,
            services,
            #[cfg(any(feature = "tonic", feature = "tonic-client"))]
            requested_service_packages,
            shared_extern_paths,
        })
    }

    fn resolve_source(source: &ProtoSource) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        match source {
            ProtoSource::Directory(dir) => {
                let mut protos = Vec::new();
                discover_proto_files(dir, &mut protos)?;
                protos.sort();
                if protos.is_empty() {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        format!("no .proto files found in directory: {}", dir.display()),
                    ));
                }
                Ok((protos, vec![dir.clone()]))
            }
            ProtoSource::Files { protos, includes } => {
                if protos.is_empty() {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "no proto files specified",
                    ));
                }
                Ok((protos.clone(), includes.clone()))
            }
        }
    }

    fn generate_buffa_messages(
        &self,
        context: &GenerationContext,
        include_entries: &mut Vec<IncludeEntry>,
    ) -> Result<()> {
        let requested_files: Vec<&str> = context
            .requested_descriptor_files
            .iter()
            .map(String::as_str)
            .collect();

        let mut config = buffa_build::Config::new();
        if let Some(config_fn) = &self.buffa_config {
            config = config_fn(config);
        }

        config = config
            .descriptor_set(&context.descriptor_path)
            .files(&requested_files)
            .out_dir(&context.out_dir)
            .generate_json(true);

        for (proto_path, rust_path) in &context.shared_extern_paths {
            config = config.extern_path(proto_path, rust_path);
        }

        config
            .compile()
            .map_err(|e| Error::other(format!("buffa-build failed: {e}")))?;

        for descriptor_name in &context.requested_descriptor_files {
            let file_name = proto_path_to_rust_module(descriptor_name);
            let package = context
                .requested_file_packages
                .get(descriptor_name)
                .cloned()
                .unwrap_or_default();

            include_entries.push(IncludeEntry { file_name, package });
        }

        Ok(())
    }

    fn generate_view_glue(
        &self,
        context: &GenerationContext,
        include_entries: &mut Vec<IncludeEntry>,
    ) -> Result<()> {
        let files_by_name: HashMap<_, _> = context
            .requested_fds
            .file
            .iter()
            .filter_map(|file| file.name.as_ref().map(|name| (name.as_str(), file)))
            .collect();

        for descriptor_name in &context.requested_descriptor_files {
            let Some(file) = files_by_name.get(descriptor_name.as_str()) else {
                continue;
            };

            let messages = collect_file_messages(file);
            if messages.is_empty() {
                continue;
            }

            let file_name = view_glue_file_name(descriptor_name);
            let file_path = context.out_dir.join(&file_name);
            let mut output = String::from("// @generated by connectrpc-axum-build\n");

            for message in messages {
                output.push_str(&format!(
                    "impl ::connectrpc_axum::HasView for {} {{ type View<'a> = {}; }}\n",
                    message.rust_path, message.view_rust_path
                ));
            }

            std::fs::write(&file_path, output)?;
            include_entries.push(IncludeEntry {
                file_name,
                package: context
                    .requested_file_packages
                    .get(descriptor_name)
                    .cloned()
                    .unwrap_or_default(),
            });
        }

        Ok(())
    }

    fn generate_connect_sidecars(
        &self,
        context: &GenerationContext,
        services: &[ProtoService],
        include_connect_server: bool,
        include_tonic: bool,
        include_connect_client: bool,
        include_entries: &mut Vec<IncludeEntry>,
    ) -> Result<()> {
        let generator = AxumConnectServiceGenerator::new()
            .with_connect_server(include_connect_server)
            .with_tonic(include_tonic)
            .with_tonic_request_mode(self.tonic_request_mode)
            .with_connect_client(include_connect_client);

        let mut services_by_package: BTreeMap<String, Vec<&ProtoService>> = BTreeMap::new();
        for service in services {
            services_by_package
                .entry(service.package.clone())
                .or_default()
                .push(service);
        }

        for (package, package_services) in services_by_package {
            let file_name = sidecar_file_name(&package, "connect");
            let file_path = context.out_dir.join(&file_name);
            let mut output = String::from("// @generated by connectrpc-axum-build\n");

            for service in package_services {
                generator.render_service(service, &mut output);
                output.push('\n');
            }

            std::fs::write(&file_path, output)?;
            include_entries.push(IncludeEntry { file_name, package });
        }

        Ok(())
    }

    #[cfg(any(feature = "tonic", feature = "tonic-client"))]
    fn generate_tonic_sidecars(
        &self,
        context: &GenerationContext,
        build_server: bool,
        build_client: bool,
        suffix: &str,
        config_fn: Option<&Box<TonicConfigFn>>,
        include_entries: &mut Vec<IncludeEntry>,
    ) -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let mut builder = tonic_buffa_build::configure();

        if let Some(config_fn) = config_fn {
            builder = config_fn(builder);
        }

        builder = builder
            .build_server(build_server)
            .build_client(build_client)
            .server_request_mode(match self.tonic_request_mode {
                TonicRequestMode::Owned => tonic_buffa_build::ServerRequestMode::Owned,
                TonicRequestMode::View => tonic_buffa_build::ServerRequestMode::View,
            })
            .compile_well_known_types(true)
            .codec_path("connectrpc_axum::tonic::BuffaCodec")
            .out_dir(temp_dir.path());

        for (proto_path, rust_path) in &context.shared_extern_paths {
            builder = builder.extern_path(proto_path, rust_path);
        }

        builder.compile_fds(context.requested_fds.clone())?;

        for package in &context.requested_service_packages {
            let generated_file_name = package_file_name(package);
            let generated_path = temp_dir.path().join(&generated_file_name);
            if !generated_path.exists() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!(
                        "tonic generation did not produce expected file '{}'",
                        generated_path.display()
                    ),
                ));
            }

            let sidecar_name = sidecar_file_name(package, suffix);
            std::fs::copy(&generated_path, context.out_dir.join(&sidecar_name))?;
            include_entries.push(IncludeEntry {
                file_name: sidecar_name,
                package: package.to_string(),
            });
        }

        Ok(())
    }
}

pub fn builder() -> CompileBuilder {
    CompileBuilder {
        source: NoSource,
        out_dir: None,
        include_file: None,
        extern_paths: Vec::new(),
        extern_reexports: Vec::new(),
        #[cfg(feature = "fetch-protoc")]
        protoc_path: None,
        buffa_config: None,
        #[cfg(feature = "tonic")]
        tonic_config: None,
        tonic_request_mode: TonicRequestMode::Owned,
        #[cfg(feature = "tonic-client")]
        tonic_client_config: None,
        _marker: PhantomData,
    }
}

pub fn compile_dir(dir: impl AsRef<Path>) -> CompileBuilder<WithSource> {
    builder().compile_dir(dir)
}

pub fn compile_protos<P: AsRef<Path>>(protos: &[P], includes: &[P]) -> CompileBuilder<WithSource> {
    builder().compile_protos(protos, includes)
}

fn out_dir_env() -> Result<String> {
    std::env::var("OUT_DIR").map_err(|e| Error::other(format!("OUT_DIR not set: {e}")))
}

fn normalize_requested_files(proto_files: &[PathBuf], includes: &[PathBuf]) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut requested = Vec::new();

    for proto_file in proto_files {
        let name = proto_relative_name(proto_file, includes);
        if name.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "failed to derive descriptor-relative name for {}",
                    proto_file.display()
                ),
            ));
        }
        if seen.insert(name.clone()) {
            requested.push(name);
        }
    }

    Ok(requested)
}

fn prune_descriptor_set(
    full_fds: &FileDescriptorSet,
    requested_files: &HashSet<String>,
) -> Result<FileDescriptorSet> {
    let files_by_name: HashMap<_, _> = full_fds
        .file
        .iter()
        .filter_map(|file| file.name.as_ref().map(|name| (name.clone(), file)))
        .collect();

    let mut needed = HashSet::new();
    let mut stack: Vec<_> = requested_files.iter().cloned().collect();

    while let Some(name) = stack.pop() {
        if !needed.insert(name.clone()) {
            continue;
        }

        let file = files_by_name.get(&name).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                format!("requested descriptor file '{name}' not found"),
            )
        })?;

        for dependency in &file.dependency {
            stack.push(dependency.clone());
        }
    }

    let files = full_fds
        .file
        .iter()
        .filter_map(|file| {
            let name = file.name.clone()?;
            if !needed.contains(&name) {
                return None;
            }
            let mut file = file.clone();
            if !requested_files.contains(&name) {
                file.service.clear();
            }
            Some(file)
        })
        .collect();

    Ok(FileDescriptorSet { file: files })
}

fn effective_shared_extern_paths(
    full_fds: &FileDescriptorSet,
    requested_files: &HashSet<String>,
    user_paths: &[(String, String)],
) -> Vec<(String, String)> {
    let mut paths = user_paths.to_vec();
    let has_wkt_mapping = paths
        .iter()
        .any(|(proto_path, _)| proto_path == ".google.protobuf");
    let generates_google_wkts = full_fds.file.iter().any(|file| {
        file.name
            .as_ref()
            .is_some_and(|name| requested_files.contains(name))
            && file.package.as_deref() == Some("google.protobuf")
    });

    if !has_wkt_mapping && !generates_google_wkts {
        paths.push((
            ".google.protobuf".to_string(),
            "::buffa_types::google::protobuf".to_string(),
        ));
    }

    paths
}

fn normalize_proto_path(mut proto_path: String) -> String {
    if !proto_path.starts_with('.') {
        proto_path.insert(0, '.');
    }
    proto_path
}

fn package_file_name(package: &str) -> String {
    let stem = if package.is_empty() {
        "_".to_string()
    } else {
        package
            .split('.')
            .filter(|segment| !segment.is_empty())
            .map(|segment| segment.to_case(Case::Snake))
            .collect::<Vec<_>>()
            .join(".")
    };
    format!("{stem}.rs")
}

fn sidecar_file_name(package: &str, suffix: &str) -> String {
    let stem = package_file_name(package);
    let stem = stem.strip_suffix(".rs").unwrap_or(&stem);
    format!("{stem}.{suffix}.rs")
}

fn proto_path_to_rust_module(proto_path: &str) -> String {
    let without_ext = proto_path.strip_suffix(".proto").unwrap_or(proto_path);
    format!("{}.rs", without_ext.replace('/', "."))
}

fn view_glue_file_name(proto_path: &str) -> String {
    let stem = proto_path_to_rust_module(proto_path);
    let stem = stem.strip_suffix(".rs").unwrap_or(&stem);
    format!("{stem}.view.rs")
}

fn proto_relative_name(file: &Path, includes: &[PathBuf]) -> String {
    let mut best: Option<&Path> = None;
    for include in includes {
        if let Ok(relative) = file.strip_prefix(include) {
            match best {
                Some(previous) if previous.as_os_str().len() <= relative.as_os_str().len() => {}
                _ => best = Some(relative),
            }
        }
    }

    best.unwrap_or(file).to_str().unwrap_or("").to_string()
}

fn run_protoc_descriptor_set(
    files: &[PathBuf],
    includes: &[PathBuf],
    #[cfg(feature = "fetch-protoc")] protoc_path: Option<&PathBuf>,
    #[cfg(not(feature = "fetch-protoc"))] _protoc_path: Option<&PathBuf>,
) -> Result<FileDescriptorSet> {
    let protoc = {
        #[cfg(feature = "fetch-protoc")]
        {
            protoc_path
                .map(|path| path.to_string_lossy().to_string())
                .or_else(|| std::env::var("PROTOC").ok())
                .unwrap_or_else(|| "protoc".to_string())
        }
        #[cfg(not(feature = "fetch-protoc"))]
        {
            std::env::var("PROTOC").unwrap_or_else(|_| "protoc".to_string())
        }
    };

    let temp_dir = tempfile::tempdir()?;
    let out_path = temp_dir.path().join("descriptor.bin");

    let mut command = Command::new(&protoc);
    command.arg("--include_imports");
    command.arg("--include_source_info");
    command.arg(format!("--descriptor_set_out={}", out_path.display()));

    for include in includes {
        command.arg(format!("--proto_path={}", include.display()));
    }
    for file in files {
        command.arg(file);
    }

    let output = command
        .output()
        .map_err(|e| Error::other(format!("failed to spawn protoc ('{protoc}'): {e}")))?;
    if !output.status.success() {
        return Err(Error::other(format!(
            "protoc failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let descriptor_bytes = std::fs::read(out_path)?;
    FileDescriptorSet::decode(descriptor_bytes.as_slice())
        .map_err(|e| Error::other(format!("failed to decode FileDescriptorSet: {e}")))
}

fn discover_proto_files(dir: &Path, proto_files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("directory not found: {}", dir.display()),
        ));
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("proto") {
            proto_files.push(path);
        } else if path.is_dir() {
            discover_proto_files(&path, proto_files)?;
        }
    }

    Ok(())
}
