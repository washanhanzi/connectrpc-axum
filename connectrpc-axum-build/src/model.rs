use convert_case::{Case, Casing};
use prost_types::{
    DescriptorProto, EnumDescriptorProto, FileDescriptorProto, FileDescriptorSet,
    MethodDescriptorProto, MethodOptions, ServiceDescriptorProto, ServiceOptions,
};
use std::collections::{HashMap, HashSet};
use std::io::{Error, ErrorKind, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtoComments {
    pub leading: String,
    pub trailing: String,
    pub detached: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoTypeRef {
    pub proto_path: String,
    pub rust_path: String,
}

#[derive(Debug, Clone)]
// TODO: keep descriptor metadata on the model for future doc/comment generation.
#[allow(dead_code)]
pub struct ProtoMethod {
    pub name: String,
    pub proto_name: String,
    pub comments: ProtoComments,
    pub input_type: ProtoTypeRef,
    pub output_type: ProtoTypeRef,
    pub input_proto_type: String,
    pub output_proto_type: String,
    pub options: MethodOptions,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub deprecated: bool,
}

#[derive(Debug, Clone)]
// TODO: keep descriptor metadata on the model for future doc/comment generation.
#[allow(dead_code)]
pub struct ProtoService {
    pub name: String,
    pub proto_name: String,
    pub package: String,
    pub comments: ProtoComments,
    pub methods: Vec<ProtoMethod>,
    pub options: ServiceOptions,
    pub deprecated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoMessage {
    pub rust_path: String,
    pub view_rust_path: String,
}

#[derive(Debug, Clone)]
struct TypeDef {
    package: String,
    parents: Vec<String>,
    name: String,
}

pub fn build_services(
    fds: &FileDescriptorSet,
    requested_files: &HashSet<String>,
    extern_paths: &[(String, String)],
) -> Result<Vec<ProtoService>> {
    let type_index = build_type_index(fds);
    let mut services = Vec::new();

    for file in &fds.file {
        let Some(file_name) = file.name.as_ref() else {
            continue;
        };
        if !requested_files.contains(file_name) {
            continue;
        }

        let package = file.package.clone().unwrap_or_default();
        for (service_index, service) in file.service.iter().enumerate() {
            services.push(build_service(
                file,
                service_index,
                service,
                &package,
                extern_paths,
                &type_index,
            )?);
        }
    }

    services.sort_by(|left, right| {
        (left.package.as_str(), left.proto_name.as_str())
            .cmp(&(right.package.as_str(), right.proto_name.as_str()))
    });
    Ok(services)
}

pub fn collect_file_messages(file: &FileDescriptorProto) -> Vec<ProtoMessage> {
    let package = file.package.as_deref().unwrap_or("");
    let mut messages = Vec::new();

    for message in &file.message_type {
        collect_message(package, message, &mut Vec::new(), &mut messages);
    }

    messages
}

fn build_service(
    file: &FileDescriptorProto,
    service_index: usize,
    service: &ServiceDescriptorProto,
    package: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<ProtoService> {
    let proto_name = service
        .name
        .clone()
        .ok_or_else(|| Error::other("service missing name"))?;
    let comments = service_comments(file, service_index);
    let options = service.options.clone().unwrap_or_default();

    let methods = service
        .method
        .iter()
        .enumerate()
        .map(|(method_index, method)| {
            build_method(
                file,
                service_index,
                method_index,
                method,
                package,
                extern_paths,
                type_index,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ProtoService {
        name: proto_name.clone(),
        proto_name,
        package: package.to_string(),
        comments,
        methods,
        deprecated: options.deprecated(),
        options,
    })
}

fn build_method(
    file: &FileDescriptorProto,
    service_index: usize,
    method_index: usize,
    method: &MethodDescriptorProto,
    current_package: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<ProtoMethod> {
    let proto_name = method
        .name
        .clone()
        .ok_or_else(|| Error::other("method missing name"))?;
    let input_proto_path = method
        .input_type
        .clone()
        .ok_or_else(|| Error::other(format!("method {proto_name} missing input type")))?;
    let output_proto_path = method
        .output_type
        .clone()
        .ok_or_else(|| Error::other(format!("method {proto_name} missing output type")))?;
    let options = method.options.clone().unwrap_or_default();

    Ok(ProtoMethod {
        name: proto_name.to_case(Case::Snake),
        proto_name,
        comments: method_comments(file, service_index, method_index),
        input_proto_type: proto_type_name(&input_proto_path),
        output_proto_type: proto_type_name(&output_proto_path),
        input_type: resolve_type_ref(current_package, &input_proto_path, extern_paths, type_index)?,
        output_type: resolve_type_ref(
            current_package,
            &output_proto_path,
            extern_paths,
            type_index,
        )?,
        options,
        client_streaming: method.client_streaming.unwrap_or(false),
        server_streaming: method.server_streaming.unwrap_or(false),
        deprecated: method
            .options
            .as_ref()
            .is_some_and(|method_options| method_options.deprecated()),
    })
}

fn resolve_type_ref(
    current_package: &str,
    proto_fqn: &str,
    extern_paths: &[(String, String)],
    type_index: &HashMap<String, TypeDef>,
) -> Result<ProtoTypeRef> {
    if let Some(extern_path) = resolve_extern_path(proto_fqn, extern_paths) {
        return Ok(ProtoTypeRef {
            proto_path: proto_fqn.to_string(),
            rust_path: extern_path,
        });
    }

    let ty = type_index.get(proto_fqn).ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!("type {proto_fqn} not found in descriptor set"),
        )
    })?;

    Ok(ProtoTypeRef {
        proto_path: proto_fqn.to_string(),
        rust_path: rust_relative_type_path(current_package, &ty.package, &ty.parents, &ty.name),
    })
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

fn collect_message(
    package: &str,
    message: &DescriptorProto,
    parents: &mut Vec<String>,
    out: &mut Vec<ProtoMessage>,
) {
    let Some(name) = message.name.as_ref() else {
        return;
    };

    if !is_map_entry(message) {
        let rust_path = rust_relative_type_path(package, package, parents, name);
        out.push(ProtoMessage {
            rust_path: rust_path.clone(),
            view_rust_path: format!("{rust_path}View<'a>"),
        });
    }

    parents.push(name.clone());
    for nested in &message.nested_type {
        collect_message(package, nested, parents, out);
    }
    parents.pop();
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

fn proto_type_name(proto_fqn: &str) -> String {
    proto_fqn
        .trim_start_matches('.')
        .split('.')
        .next_back()
        .unwrap_or_default()
        .to_string()
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

fn service_comments(file: &FileDescriptorProto, service_index: usize) -> ProtoComments {
    comments_for_path(file, &[6, service_index as i32])
}

fn method_comments(
    file: &FileDescriptorProto,
    service_index: usize,
    method_index: usize,
) -> ProtoComments {
    comments_for_path(file, &[6, service_index as i32, 2, method_index as i32])
}

fn comments_for_path(file: &FileDescriptorProto, path: &[i32]) -> ProtoComments {
    let Some(source_info) = file.source_code_info.as_ref() else {
        return ProtoComments::default();
    };

    source_info
        .location
        .iter()
        .find(|location| location.path == path)
        .map(|location| ProtoComments {
            leading: location.leading_comments.clone().unwrap_or_default(),
            trailing: location.trailing_comments.clone().unwrap_or_default(),
            detached: location.leading_detached_comments.clone(),
        })
        .unwrap_or_default()
}
