use convert_case::{Case, Casing};
use include_file::IncludeEntry;
use proc_macro2::TokenStream;
use prost::Message as _;
use prost_types::{
    DescriptorProto, EnumDescriptorProto, FileDescriptorProto, FileDescriptorSet,
    MethodDescriptorProto, ServiceDescriptorProto,
};
use quote::quote;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::parse2;
use tonic_build::{CodeGenBuilder, Method as TonicMethod, Service as TonicService};

mod include_file;

#[derive(Debug, Clone)]
struct ServiceDef {
    name: String,
    proto_name: String,
    package: String,
    methods: Vec<MethodDef>,
}

#[derive(Debug, Clone)]
struct MethodDef {
    name: String,
    proto_name: String,
    input_type: String,
    output_type: String,
    client_streaming: bool,
    server_streaming: bool,
    codec_path: String,
    deprecated: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ServerRequestMode {
    #[default]
    Owned,
    View,
}

#[derive(Debug, Clone)]
struct TypeDef {
    package: String,
    parents: Vec<String>,
    name: String,
}

#[derive(Debug, Clone)]
struct PreparedServiceDef {
    name: String,
    proto_name: String,
    package: String,
    methods: Vec<PreparedMethodDef>,
}

#[derive(Debug, Clone)]
struct PreparedMethodDef {
    name: String,
    proto_name: String,
    request_tokens: TokenStream,
    response_tokens: TokenStream,
    client_streaming: bool,
    server_streaming: bool,
    codec_path: String,
    deprecated: bool,
}

impl TonicService for PreparedServiceDef {
    type Method = PreparedMethodDef;
    type Comment = String;

    fn name(&self) -> &str {
        &self.name
    }

    fn package(&self) -> &str {
        &self.package
    }

    fn identifier(&self) -> &str {
        &self.proto_name
    }

    fn methods(&self) -> &[Self::Method] {
        &self.methods
    }

    fn comment(&self) -> &[Self::Comment] {
        &[]
    }
}

impl TonicMethod for PreparedMethodDef {
    type Comment = String;

    fn name(&self) -> &str {
        &self.name
    }

    fn identifier(&self) -> &str {
        &self.proto_name
    }

    fn codec_path(&self) -> &str {
        &self.codec_path
    }

    fn client_streaming(&self) -> bool {
        self.client_streaming
    }

    fn server_streaming(&self) -> bool {
        self.server_streaming
    }

    fn comment(&self) -> &[Self::Comment] {
        &[]
    }

    fn deprecated(&self) -> bool {
        self.deprecated
    }

    fn request_response_name(
        &self,
        _proto_path: &str,
        _compile_well_known_types: bool,
    ) -> (TokenStream, TokenStream) {
        (self.request_tokens.clone(), self.response_tokens.clone())
    }
}

fn prepare_type_path(path: &str, proto_path: &str) -> Result<TokenStream> {
    const NON_PATH_TYPE_ALLOWLIST: &[&str] = &["()"];

    if NON_PATH_TYPE_ALLOWLIST.iter().any(|ty| path.ends_with(ty)) {
        return syn::parse_str::<syn::Type>(path)
            .map(|ty| quote!(#ty))
            .map_err(|e| {
                Error::other(format!("invalid tonic Rust type expression '{path}': {e}"))
            });
    }

    if path.starts_with("::") || path.starts_with("crate::") {
        return syn::parse_str::<syn::Type>(path)
            .map(|ty| quote!(#ty))
            .map_err(|e| Error::other(format!("invalid tonic Rust type path '{path}': {e}")));
    }

    let rust_type = path.trim_start_matches("::");
    let full_path = format!("{proto_path}::{rust_type}");
    syn::parse_str::<syn::Type>(&full_path)
        .map(|ty| quote!(#ty))
        .map_err(|e| Error::other(format!("invalid tonic Rust type path '{full_path}': {e}")))
}

#[derive(Debug, Clone)]
pub struct Builder {
    build_client: bool,
    build_server: bool,
    build_transport: bool,
    codec_path: String,
    compile_well_known_types: bool,
    emit_package: bool,
    include_file: Option<String>,
    out_dir: Option<PathBuf>,
    proto_path: String,
    extern_paths: Vec<(String, String)>,
    server_request_mode: ServerRequestMode,
    view_wrapper_path: String,
    use_arc_self: bool,
    generate_default_stubs: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            build_client: true,
            build_server: true,
            build_transport: true,
            codec_path: "tonic_prost::ProstCodec".to_string(),
            compile_well_known_types: false,
            emit_package: true,
            include_file: None,
            out_dir: None,
            proto_path: "super".to_string(),
            extern_paths: Vec::new(),
            server_request_mode: ServerRequestMode::Owned,
            view_wrapper_path: "::connectrpc_axum::View".to_string(),
            use_arc_self: false,
            generate_default_stubs: false,
        }
    }
}

pub fn configure() -> Builder {
    Builder::default()
}

impl Builder {
    pub fn build_client(mut self, enable: bool) -> Self {
        self.build_client = enable;
        self
    }

    pub fn build_server(mut self, enable: bool) -> Self {
        self.build_server = enable;
        self
    }

    pub fn build_transport(mut self, enable: bool) -> Self {
        self.build_transport = enable;
        self
    }

    pub fn codec_path(mut self, path: impl AsRef<str>) -> Self {
        self.codec_path = path.as_ref().to_string();
        self
    }

    pub fn compile_well_known_types(mut self, enable: bool) -> Self {
        self.compile_well_known_types = enable;
        self
    }

    pub fn emit_package(mut self, enable: bool) -> Self {
        self.emit_package = enable;
        self
    }

    pub fn include_file(mut self, path: impl AsRef<str>) -> Self {
        self.include_file = Some(path.as_ref().to_string());
        self
    }

    pub fn out_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.out_dir = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn proto_path(mut self, path: impl AsRef<str>) -> Self {
        self.proto_path = path.as_ref().to_string();
        self
    }

    pub fn extern_path(mut self, proto_path: impl AsRef<str>, rust_path: impl AsRef<str>) -> Self {
        self.extern_paths.push((
            normalize_proto_path(proto_path.as_ref().to_string()),
            rust_path.as_ref().to_string(),
        ));
        self
    }

    pub fn server_request_mode(mut self, mode: ServerRequestMode) -> Self {
        self.server_request_mode = mode;
        self
    }

    pub fn view_wrapper_path(mut self, path: impl AsRef<str>) -> Self {
        let path = path.as_ref();
        self.view_wrapper_path = if path.starts_with("::") {
            path.to_string()
        } else {
            format!("::{path}")
        };
        self
    }

    pub fn use_arc_self(mut self, enable: bool) -> Self {
        self.use_arc_self = enable;
        self
    }

    pub fn generate_default_stubs(mut self, enable: bool) -> Self {
        self.generate_default_stubs = enable;
        self
    }

    pub fn compile_fds(self, fds: FileDescriptorSet) -> Result<()> {
        self.compile_fds_filtered(fds, None)
    }

    pub fn compile_protos<P: AsRef<Path>>(self, protos: &[P], includes: &[P]) -> Result<()> {
        if protos.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "no proto files specified",
            ));
        }

        let descriptor_bytes = run_protoc(
            &protos
                .iter()
                .map(|p| p.as_ref().to_path_buf())
                .collect::<Vec<_>>(),
            &includes
                .iter()
                .map(|p| p.as_ref().to_path_buf())
                .collect::<Vec<_>>(),
        )?;
        let fds = FileDescriptorSet::decode(descriptor_bytes.as_slice())
            .map_err(|e| Error::other(format!("failed to decode FileDescriptorSet: {e}")))?;
        let requested_files = normalize_requested_files(
            &protos
                .iter()
                .map(|p| p.as_ref().to_path_buf())
                .collect::<Vec<_>>(),
            &includes
                .iter()
                .map(|p| p.as_ref().to_path_buf())
                .collect::<Vec<_>>(),
        )?;

        self.compile_fds_filtered(fds, Some(requested_files))
    }

    fn compile_fds_filtered(
        self,
        fds: FileDescriptorSet,
        requested_files: Option<Vec<String>>,
    ) -> Result<()> {
        let out_dir = self.resolve_out_dir()?;
        fs::create_dir_all(&out_dir)?;

        let type_index = build_type_index(&fds);
        let extern_paths = effective_extern_paths(&self.extern_paths);
        let requested_files = requested_files.unwrap_or_else(|| {
            fds.file
                .iter()
                .filter_map(|file| file.name.clone())
                .collect::<Vec<_>>()
        });

        let mut services_by_package: BTreeMap<String, Vec<ServiceDef>> = BTreeMap::new();

        for file in &fds.file {
            let Some(file_name) = file.name.as_ref() else {
                continue;
            };
            if !requested_files
                .iter()
                .any(|requested| requested == file_name)
            {
                continue;
            }

            let package = file.package.clone().unwrap_or_default();
            let service_defs = build_services_for_file(
                file,
                &package,
                &self.codec_path,
                &extern_paths,
                &type_index,
            )?;

            if !service_defs.is_empty() {
                services_by_package
                    .entry(package)
                    .or_default()
                    .extend(service_defs);
            }
        }

        let mut include_entries = Vec::new();

        for (package, services) in services_by_package {
            let file_name = package_file_name(&package);
            let file_path = out_dir.join(&file_name);
            let mut output = String::from("// @generated by tonic-buffa-build\n");

            for service in services {
                let tokens = self.generate_service_tokens(&service)?;
                output.push_str(&format_tokens(tokens)?);
                output.push('\n');
            }

            fs::write(file_path, output)?;
            include_entries.push(IncludeEntry { file_name, package });
        }

        if let Some(include_name) = &self.include_file {
            include_file::generate(
                include_name,
                out_dir
                    .to_str()
                    .ok_or_else(|| Error::other("invalid output directory"))?,
                &include_entries,
                self.out_dir.is_none(),
            )?;
        }

        Ok(())
    }

    fn resolve_out_dir(&self) -> Result<PathBuf> {
        match &self.out_dir {
            Some(path) => Ok(path.clone()),
            None => std::env::var_os("OUT_DIR")
                .map(PathBuf::from)
                .ok_or_else(|| Error::other("OUT_DIR not set")),
        }
    }

    fn generate_service_tokens(&self, service: &ServiceDef) -> Result<TokenStream> {
        let mut builder = CodeGenBuilder::new();
        builder
            .emit_package(self.emit_package)
            .compile_well_known_types(self.compile_well_known_types)
            .build_transport(self.build_transport)
            .use_arc_self(self.use_arc_self)
            .generate_default_stubs(self.generate_default_stubs);

        let mut tokens = TokenStream::new();

        if self.build_client {
            let prepared = prepare_service(service, &self.proto_path)?;
            tokens.extend(builder.generate_client(&prepared, &self.proto_path));
        }

        if self.build_server {
            let server_service =
                service.with_server_request_mode(self.server_request_mode, &self.view_wrapper_path);
            let prepared = prepare_service(&server_service, &self.proto_path)?;
            tokens.extend(builder.generate_server(&prepared, &self.proto_path));
        }

        Ok(tokens)
    }
}

impl ServiceDef {
    fn with_server_request_mode(&self, mode: ServerRequestMode, view_wrapper_path: &str) -> Self {
        if mode == ServerRequestMode::Owned {
            return self.clone();
        }

        let mut service = self.clone();
        for method in &mut service.methods {
            method.input_type = format!("{view_wrapper_path}<{}>", method.input_type);
        }
        service
    }
}

fn build_services_for_file(
    file: &FileDescriptorProto,
    package: &str,
    codec_path: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<Vec<ServiceDef>> {
    file.service
        .iter()
        .map(|service| build_service(service, package, codec_path, extern_paths, type_index))
        .collect()
}

fn build_service(
    service: &ServiceDescriptorProto,
    package: &str,
    codec_path: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<ServiceDef> {
    let proto_name = service
        .name
        .clone()
        .ok_or_else(|| Error::other("service missing name"))?;

    let methods = service
        .method
        .iter()
        .map(|method| build_method(method, package, codec_path, extern_paths, type_index))
        .collect::<Result<Vec<_>>>()?;

    Ok(ServiceDef {
        name: proto_name.clone(),
        proto_name,
        package: package.to_string(),
        methods,
    })
}

fn prepare_service(service: &ServiceDef, proto_path: &str) -> Result<PreparedServiceDef> {
    Ok(PreparedServiceDef {
        name: service.name.clone(),
        proto_name: service.proto_name.clone(),
        package: service.package.clone(),
        methods: service
            .methods
            .iter()
            .map(|method| prepare_method(method, proto_path))
            .collect::<Result<Vec<_>>>()?,
    })
}

fn prepare_method(method: &MethodDef, proto_path: &str) -> Result<PreparedMethodDef> {
    Ok(PreparedMethodDef {
        name: method.name.clone(),
        proto_name: method.proto_name.clone(),
        request_tokens: prepare_type_path(&method.input_type, proto_path)?,
        response_tokens: prepare_type_path(&method.output_type, proto_path)?,
        client_streaming: method.client_streaming,
        server_streaming: method.server_streaming,
        codec_path: method.codec_path.clone(),
        deprecated: method.deprecated,
    })
}

fn build_method(
    method: &MethodDescriptorProto,
    current_package: &str,
    codec_path: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<MethodDef> {
    let proto_name = method
        .name
        .clone()
        .ok_or_else(|| Error::other("method missing name"))?;
    let input_type = method
        .input_type
        .as_deref()
        .ok_or_else(|| Error::other(format!("method {proto_name} missing input type")))?;
    let output_type = method
        .output_type
        .as_deref()
        .ok_or_else(|| Error::other(format!("method {proto_name} missing output type")))?;

    Ok(MethodDef {
        name: proto_name.to_case(Case::Snake),
        proto_name,
        input_type: resolve_type_path(current_package, input_type, extern_paths, type_index)?,
        output_type: resolve_type_path(current_package, output_type, extern_paths, type_index)?,
        client_streaming: method.client_streaming.unwrap_or(false),
        server_streaming: method.server_streaming.unwrap_or(false),
        codec_path: codec_path.to_string(),
        deprecated: method
            .options
            .as_ref()
            .is_some_and(|options| options.deprecated()),
    })
}

fn resolve_type_path(
    current_package: &str,
    proto_fqn: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<String> {
    if let Some(extern_path) = resolve_extern_path(proto_fqn, extern_paths) {
        return Ok(extern_path);
    }

    let ty = type_index.get(proto_fqn).ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!("type {proto_fqn} not found in descriptor set"),
        )
    })?;

    Ok(rust_relative_type_path(
        current_package,
        &ty.package,
        &ty.parents,
        &ty.name,
    ))
}

fn resolve_extern_path(proto_fqn: &str, extern_paths: &[(String, String)]) -> Option<String> {
    let matched = extern_paths
        .iter()
        .filter(|(prefix, _)| is_prefix_match(proto_fqn, prefix))
        .max_by_key(|(prefix, _)| prefix.len())?;

    let (prefix, rust_root) = matched;
    let suffix = proto_fqn.trim_start_matches('.');
    let prefix_len = prefix
        .trim_start_matches('.')
        .split('.')
        .filter(|s| !s.is_empty())
        .count();
    let suffix_segments = suffix.split('.').skip(prefix_len).collect::<Vec<_>>();

    if suffix_segments.is_empty() {
        return Some(rust_root.clone());
    }

    let mut rust_path = rust_root.clone();
    for (index, segment) in suffix_segments.iter().enumerate() {
        if !rust_path.ends_with("::") {
            rust_path.push_str("::");
        }
        if index == suffix_segments.len() - 1 {
            rust_path.push_str(&segment.to_case(Case::UpperCamel));
        } else {
            rust_path.push_str(&segment.to_case(Case::Snake));
        }
    }

    Some(rust_path)
}

fn is_prefix_match(proto_fqn: &str, prefix: &str) -> bool {
    if prefix == "." {
        return true;
    }

    proto_fqn == prefix
        || proto_fqn
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn effective_extern_paths(user_paths: &[(String, String)]) -> Vec<(String, String)> {
    let mut paths = user_paths.to_vec();
    let has_wkt_mapping = paths
        .iter()
        .any(|(proto_path, _)| proto_path == ".google.protobuf");

    if !has_wkt_mapping {
        paths.push((
            ".google.protobuf".to_string(),
            "::buffa_types::google::protobuf".to_string(),
        ));
    }

    paths
}

fn build_type_index(fds: &FileDescriptorSet) -> HashMap<String, TypeDef> {
    let mut index = HashMap::new();

    for file in &fds.file {
        let package = file.package.as_deref().unwrap_or("");

        for message in &file.message_type {
            collect_message_types(package, message, &mut Vec::new(), &mut index);
        }

        for enumeration in &file.enum_type {
            collect_enum_types(package, enumeration, &mut Vec::new(), &mut index);
        }
    }

    index
}

fn collect_message_types(
    package: &str,
    message: &DescriptorProto,
    parents: &mut Vec<String>,
    index: &mut HashMap<String, TypeDef>,
) {
    let Some(name) = message.name.as_ref() else {
        return;
    };

    if !is_map_entry(message) {
        index.insert(
            fq_type_name(package, parents, name),
            TypeDef {
                package: package.to_string(),
                parents: parents.clone(),
                name: name.clone(),
            },
        );
    }

    parents.push(name.clone());
    for nested in &message.nested_type {
        collect_message_types(package, nested, parents, index);
    }
    for enumeration in &message.enum_type {
        collect_enum_types(package, enumeration, parents, index);
    }
    parents.pop();
}

fn collect_enum_types(
    package: &str,
    enumeration: &EnumDescriptorProto,
    parents: &mut Vec<String>,
    index: &mut HashMap<String, TypeDef>,
) {
    let Some(name) = enumeration.name.as_ref() else {
        return;
    };

    index.insert(
        fq_type_name(package, parents, name),
        TypeDef {
            package: package.to_string(),
            parents: parents.clone(),
            name: name.clone(),
        },
    );
}

fn rust_relative_type_path(
    current_package: &str,
    target_package: &str,
    parents: &[String],
    name: &str,
) -> String {
    let current_parts = package_rust_parts(current_package);
    let target_parts = package_rust_parts(target_package);
    let common = current_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();

    let mut rust_parts = Vec::new();
    for _ in common..current_parts.len() {
        rust_parts.push("super".to_string());
    }
    rust_parts.extend(target_parts.into_iter().skip(common));
    rust_parts.extend(parents.iter().map(|segment| segment.to_case(Case::Snake)));
    rust_parts.push(name.to_case(Case::UpperCamel));
    rust_parts.join("::")
}

fn package_rust_parts(package: &str) -> Vec<String> {
    package
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_case(Case::Snake))
        .collect()
}

fn is_map_entry(message: &DescriptorProto) -> bool {
    message
        .options
        .as_ref()
        .is_some_and(|options| options.map_entry())
}

fn fq_type_name(package: &str, parents: &[String], name: &str) -> String {
    let mut parts = Vec::new();
    if !package.is_empty() {
        parts.extend(package.split('.').map(str::to_string));
    }
    parts.extend(parents.iter().cloned());
    parts.push(name.to_string());
    format!(".{}", parts.join("."))
}

fn format_tokens(tokens: TokenStream) -> Result<String> {
    let file = parse2::<syn::File>(tokens)
        .map_err(|e| Error::other(format!("generated tonic code failed to parse: {e}")))?;
    Ok(prettyplease::unparse(&file))
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

fn normalize_proto_path(mut proto_path: String) -> String {
    if !proto_path.starts_with('.') {
        proto_path.insert(0, '.');
    }
    proto_path
}

fn normalize_requested_files(proto_files: &[PathBuf], includes: &[PathBuf]) -> Result<Vec<String>> {
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
        if !requested.iter().any(|existing| existing == &name) {
            requested.push(name);
        }
    }

    Ok(requested)
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

fn run_protoc(files: &[PathBuf], includes: &[PathBuf]) -> Result<Vec<u8>> {
    let protoc = std::env::var("PROTOC").unwrap_or_else(|_| "protoc".to_string());
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

    fs::read(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::{FileDescriptorProto, MethodOptions, ServiceOptions};

    fn test_fds() -> FileDescriptorSet {
        FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("hello.proto".to_string()),
                package: Some("hello".to_string()),
                message_type: vec![
                    DescriptorProto {
                        name: Some("HelloRequest".to_string()),
                        ..Default::default()
                    },
                    DescriptorProto {
                        name: Some("HelloResponse".to_string()),
                        ..Default::default()
                    },
                ],
                service: vec![ServiceDescriptorProto {
                    name: Some("HelloWorldService".to_string()),
                    method: vec![MethodDescriptorProto {
                        name: Some("SayHello".to_string()),
                        input_type: Some(".hello.HelloRequest".to_string()),
                        output_type: Some(".hello.HelloResponse".to_string()),
                        client_streaming: Some(false),
                        server_streaming: Some(false),
                        options: Some(MethodOptions::default()),
                        ..Default::default()
                    }],
                    options: Some(ServiceOptions::default()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn generates_tonic_stub_with_extern_types() {
        let dir = tempfile::tempdir().unwrap();

        configure()
            .out_dir(dir.path())
            .codec_path("crate::codec::BuffaCodec")
            .extern_path(".hello", "crate::proto::hello")
            .compile_fds(test_fds())
            .unwrap();

        let generated = fs::read_to_string(dir.path().join("hello.rs")).unwrap();
        assert!(generated.contains("crate::codec::BuffaCodec"));
        assert!(generated.contains("tonic::Request<crate::proto::hello::HelloRequest>"));
        assert!(generated.contains("tonic::Response<crate::proto::hello::HelloResponse>"));
    }

    #[test]
    fn generates_include_file() {
        let dir = tempfile::tempdir().unwrap();

        configure()
            .out_dir(dir.path())
            .codec_path("crate::codec::BuffaCodec")
            .extern_path(".hello", "crate::proto::hello")
            .include_file("mod.rs")
            .compile_fds(test_fds())
            .unwrap();

        let generated = fs::read_to_string(dir.path().join("mod.rs")).unwrap();
        assert!(generated.contains("pub mod hello {"));
        assert!(generated.contains("include!("));
    }

    #[test]
    fn view_mode_changes_server_requests_only() {
        let dir = tempfile::tempdir().unwrap();

        configure()
            .out_dir(dir.path())
            .codec_path("crate::codec::BuffaCodec")
            .extern_path(".hello", "crate::proto::hello")
            .server_request_mode(ServerRequestMode::View)
            .compile_fds(test_fds())
            .unwrap();

        let generated = fs::read_to_string(dir.path().join("hello.rs")).unwrap();
        assert!(generated.contains("::connectrpc_axum::View<crate::proto::hello::HelloRequest>"));
        assert!(generated.contains("IntoRequest<crate::proto::hello::HelloRequest>"));
    }

    #[test]
    fn invalid_view_wrapper_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();

        let err = configure()
            .out_dir(dir.path())
            .codec_path("crate::codec::BuffaCodec")
            .extern_path(".hello", "crate::proto::hello")
            .server_request_mode(ServerRequestMode::View)
            .view_wrapper_path("not a valid path<")
            .compile_fds(test_fds())
            .unwrap_err();

        assert!(err.to_string().contains("invalid tonic Rust type"));
    }
}
