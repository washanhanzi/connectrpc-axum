use ::prost::Message;
use prost_types::{
    DescriptorProto, EnumDescriptorProto, FileDescriptorProto, FileDescriptorSet,
    MethodDescriptorProto, ServiceDescriptorProto,
};
use std::collections::{BTreeSet, HashMap};
use std::io::Result;

mod prost;

pub(crate) use prost::ProstSchemaResolver;

/// Normalized protobuf schema facts for connectrpc-axum-build.
///
/// This module is intentionally prost-centric: it normalizes descriptor data
/// once, then exposes prost-compatible type and service resolution used by the
/// Connect emitter and tonic `extern_path` wiring.
#[derive(Debug, Clone, Default)]
pub(crate) struct SchemaSet {
    pub(crate) types: TypeIndex,
    pub(crate) services: Vec<ServiceModel>,
    /// All proto packages seen in the descriptor set (including imports),
    /// in sorted order.
    pub(crate) packages: BTreeSet<String>,
}

/// Message and enum types keyed by fully-qualified proto name (e.g.
/// `.greet.v1.HelloRequest`).
#[derive(Debug, Clone, Default)]
pub(crate) struct TypeIndex {
    types: HashMap<String, TypeModel>,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeModel {
    package: String,
    scoped_name: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceModel {
    pub(crate) package: String,
    pub(crate) proto_name: String,
    pub(crate) methods: Vec<MethodModel>,
}

#[derive(Debug, Clone)]
pub(crate) struct MethodModel {
    pub(crate) proto_name: String,
    pub(crate) route_path: String,
    pub(crate) input_type: String,
    pub(crate) output_type: String,
    pub(crate) client_streaming: bool,
    pub(crate) server_streaming: bool,
    pub(crate) idempotency_level: Option<i32>,
}

#[cfg(any(test, feature = "tonic", feature = "tonic-client"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypePathMapping {
    pub(crate) proto_path: String,
    pub(crate) rust_path: String,
}

impl SchemaSet {
    pub(crate) fn from_descriptor_bytes(bytes: &[u8]) -> Result<Self> {
        let fds = FileDescriptorSet::decode(bytes)
            .map_err(|e| std::io::Error::other(format!("decode descriptor: {e}")))?;
        Ok(Self::from_file_descriptor_set(&fds))
    }

    pub(crate) fn from_file_descriptor_set(fds: &FileDescriptorSet) -> Self {
        let mut types = TypeIndex::default();
        let mut services = Vec::new();
        let mut packages = BTreeSet::new();

        for file in &fds.file {
            let package = file.package.clone().unwrap_or_default();
            packages.insert(package.clone());

            for msg in &file.message_type {
                register_message(&mut types, &package, &[], msg);
            }

            for en in &file.enum_type {
                register_enum(&mut types, &package, &[], en);
            }

            services.extend(
                file.service
                    .iter()
                    .filter_map(|service| ServiceModel::from_descriptor_service(file, service)),
            );
        }

        Self {
            types,
            services,
            packages,
        }
    }

    pub(crate) fn find_type(&self, proto_fqn: &str) -> Option<&TypeModel> {
        self.types.find(proto_fqn)
    }

    pub(crate) fn prost(&self) -> ProstSchemaResolver<'_> {
        ProstSchemaResolver::new(self)
    }
}

impl TypeIndex {
    fn find(&self, proto_fqn: &str) -> Option<&TypeModel> {
        self.types.get(proto_fqn)
    }

    #[cfg(any(test, feature = "tonic", feature = "tonic-client"))]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &TypeModel)> {
        self.types.iter().map(|(fqn, ty)| (fqn.as_str(), ty))
    }
}

impl ServiceModel {
    fn from_descriptor_service(
        file: &FileDescriptorProto,
        service: &ServiceDescriptorProto,
    ) -> Option<Self> {
        let package = file.package.clone().unwrap_or_default();
        let proto_name = service.name.clone()?;

        let methods = service
            .method
            .iter()
            .filter_map(|method| MethodModel::from_descriptor_method(&package, &proto_name, method))
            .collect();

        Some(Self {
            package,
            proto_name,
            methods,
        })
    }
}

impl MethodModel {
    fn from_descriptor_method(
        package: &str,
        service_name: &str,
        method: &MethodDescriptorProto,
    ) -> Option<Self> {
        let proto_name = method.name.clone()?;

        Some(Self {
            route_path: route_path(package, service_name, &proto_name),
            input_type: normalize_proto_type(method.input_type.as_deref()),
            output_type: normalize_proto_type(method.output_type.as_deref()),
            client_streaming: method.client_streaming.unwrap_or(false),
            server_streaming: method.server_streaming.unwrap_or(false),
            idempotency_level: method
                .options
                .as_ref()
                .and_then(|options| options.idempotency_level),
            proto_name,
        })
    }
}

fn register_message(
    types: &mut TypeIndex,
    package: &str,
    parents: &[String],
    msg: &DescriptorProto,
) {
    let Some(name) = msg.name.as_deref().filter(|name| !name.is_empty()) else {
        return;
    };

    let mut scoped_name = parents.to_vec();
    scoped_name.push(name.to_string());

    types.types.insert(
        qualified_proto_name(package, &scoped_name),
        TypeModel {
            package: package.to_string(),
            scoped_name: scoped_name.clone(),
        },
    );

    for nested in &msg.nested_type {
        register_message(types, package, &scoped_name, nested);
    }

    for en in &msg.enum_type {
        register_enum(types, package, &scoped_name, en);
    }
}

fn register_enum(
    types: &mut TypeIndex,
    package: &str,
    parents: &[String],
    en: &EnumDescriptorProto,
) {
    let Some(name) = en.name.as_deref().filter(|name| !name.is_empty()) else {
        return;
    };

    let mut scoped_name = parents.to_vec();
    scoped_name.push(name.to_string());

    types.types.insert(
        qualified_proto_name(package, &scoped_name),
        TypeModel {
            package: package.to_string(),
            scoped_name,
        },
    );
}

fn qualified_proto_name(package: &str, scoped_name: &[String]) -> String {
    let mut proto_name = String::from(".");

    if !package.is_empty() {
        proto_name.push_str(package);
        if !scoped_name.is_empty() {
            proto_name.push('.');
        }
    }

    proto_name.push_str(&scoped_name.join("."));
    proto_name
}

fn route_path(package: &str, service_name: &str, method_name: &str) -> String {
    if package.is_empty() {
        return format!("/{service_name}/{method_name}");
    }

    format!("/{package}.{service_name}/{method_name}")
}

fn normalize_proto_type(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::{FileDescriptorSet, MethodOptions};

    #[test]
    fn builds_type_index_and_services_from_descriptor_set() {
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("greet.proto".to_string()),
                package: Some("greet.v1".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("HelloRequest".to_string()),
                    nested_type: vec![DescriptorProto {
                        name: Some("Labels".to_string()),
                        ..Default::default()
                    }],
                    enum_type: vec![EnumDescriptorProto {
                        name: Some("Kind".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                enum_type: vec![EnumDescriptorProto {
                    name: Some("TopLevel".to_string()),
                    ..Default::default()
                }],
                service: vec![ServiceDescriptorProto {
                    name: Some("Greeter".to_string()),
                    method: vec![MethodDescriptorProto {
                        name: Some("SayHello".to_string()),
                        input_type: Some(".greet.v1.HelloRequest".to_string()),
                        output_type: Some(".greet.v1.HelloRequest".to_string()),
                        options: Some(MethodOptions {
                            idempotency_level: Some(1),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        let schema = SchemaSet::from_file_descriptor_set(&fds);
        let mappings = schema.prost().type_path_mappings("greet.v1");

        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".greet.v1.HelloRequest".to_string(),
            rust_path: "HelloRequest".to_string(),
        }));
        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".greet.v1.HelloRequest.Labels".to_string(),
            rust_path: "hello_request::Labels".to_string(),
        }));
        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".greet.v1.HelloRequest.Kind".to_string(),
            rust_path: "hello_request::Kind".to_string(),
        }));
        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".greet.v1.TopLevel".to_string(),
            rust_path: "TopLevel".to_string(),
        }));

        assert_eq!(schema.services.len(), 1);
        let greeter = &schema.services[0];
        assert_eq!(greeter.package, "greet.v1");
        assert_eq!(greeter.proto_name, "Greeter");
        assert_eq!(greeter.methods.len(), 1);

        let method = &greeter.methods[0];
        assert_eq!(method.proto_name, "SayHello");
        assert_eq!(method.route_path, "/greet.v1.Greeter/SayHello");
        assert_eq!(method.input_type, ".greet.v1.HelloRequest");
        assert_eq!(method.output_type, ".greet.v1.HelloRequest");
        assert_eq!(method.idempotency_level, Some(1));
    }

    #[test]
    fn supports_packageless_routes_and_type_names() {
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("root.proto".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("RootMessage".to_string()),
                    ..Default::default()
                }],
                service: vec![ServiceDescriptorProto {
                    name: Some("RootService".to_string()),
                    method: vec![MethodDescriptorProto {
                        name: Some("Call".to_string()),
                        input_type: Some(".RootMessage".to_string()),
                        output_type: Some(".RootMessage".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        let schema = SchemaSet::from_file_descriptor_set(&fds);
        let mappings = schema.prost().type_path_mappings("");

        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".RootMessage".to_string(),
            rust_path: "RootMessage".to_string(),
        }));
        assert_eq!(
            schema.services[0].methods[0].route_path,
            "/RootService/Call"
        );
    }

    #[test]
    fn prost_resolver_resolves_relative_type_paths() {
        let schema = SchemaSet::from_file_descriptor_set(&FileDescriptorSet {
            file: vec![
                FileDescriptorProto {
                    name: Some("bar.proto".to_string()),
                    package: Some("foo.bar".to_string()),
                    message_type: vec![DescriptorProto {
                        name: Some("HelloRequest".to_string()),
                        nested_type: vec![DescriptorProto {
                            name: Some("Labels".to_string()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                FileDescriptorProto {
                    name: Some("common.proto".to_string()),
                    package: Some("foo.common".to_string()),
                    message_type: vec![DescriptorProto {
                        name: Some("Shared".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
        });

        let prost = schema.prost();

        assert_eq!(
            prost.rust_type_relative(".foo.bar.HelloRequest", "foo.bar", 0),
            Some("HelloRequest".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".foo.bar.HelloRequest", "foo.bar", 1),
            Some("super::HelloRequest".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".foo.bar.HelloRequest.Labels", "foo.bar", 0),
            Some("hello_request::Labels".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".foo.bar.HelloRequest.Labels", "foo.bar", 1),
            Some("super::hello_request::Labels".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".foo.common.Shared", "foo.bar", 0),
            Some("super::common::Shared".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".foo.common.Shared", "foo.bar", 1),
            Some("super::super::common::Shared".to_string())
        );
    }

    #[test]
    fn prost_resolver_maps_well_known_types_to_prost_defaults() {
        let schema = SchemaSet::default();
        let prost = schema.prost();

        assert_eq!(
            prost.rust_type_relative(".google.protobuf.Empty", "greet.v1", 1),
            Some("()".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".google.protobuf.Timestamp", "greet.v1", 0),
            Some("::prost_types::Timestamp".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".google.protobuf.Field.Kind", "", 0),
            Some("::prost_types::field::Kind".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".google.protobuf.StringValue", "greet.v1", 2),
            Some("::prost::alloc::string::String".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".google.protobuf.BytesValue", "greet.v1", 0),
            Some("::prost::alloc::vec::Vec<u8>".to_string())
        );
        assert_eq!(
            prost.rust_type_relative(".google.protobufish.Thing", "greet.v1", 0),
            None
        );
    }

    #[test]
    fn type_path_mappings_are_package_relative_and_skip_well_known_types() {
        let schema = SchemaSet::from_file_descriptor_set(&FileDescriptorSet {
            file: vec![
                FileDescriptorProto {
                    name: Some("bar.proto".to_string()),
                    package: Some("foo.bar".to_string()),
                    message_type: vec![DescriptorProto {
                        name: Some("Local".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                FileDescriptorProto {
                    name: Some("common.proto".to_string()),
                    package: Some("foo.common".to_string()),
                    message_type: vec![DescriptorProto {
                        name: Some("Shared".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                FileDescriptorProto {
                    name: Some("google/protobuf/empty.proto".to_string()),
                    package: Some("google.protobuf".to_string()),
                    message_type: vec![DescriptorProto {
                        name: Some("Empty".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
        });

        let mappings = schema.prost().type_path_mappings("foo.bar");

        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".foo.bar.Local".to_string(),
            rust_path: "Local".to_string(),
        }));
        assert!(mappings.contains(&TypePathMapping {
            proto_path: ".foo.common.Shared".to_string(),
            rust_path: "super::common::Shared".to_string(),
        }));
        // Prost's default extern mappings already cover well-known types
        assert!(
            !mappings
                .iter()
                .any(|mapping| mapping.proto_path.starts_with(".google.protobuf"))
        );
    }

    #[test]
    fn collects_packages_from_all_files() {
        let schema = SchemaSet::from_file_descriptor_set(&FileDescriptorSet {
            file: vec![
                FileDescriptorProto {
                    name: Some("bar.proto".to_string()),
                    package: Some("foo.bar".to_string()),
                    ..Default::default()
                },
                FileDescriptorProto {
                    name: Some("root.proto".to_string()),
                    ..Default::default()
                },
            ],
        });

        assert_eq!(
            schema.packages.iter().cloned().collect::<Vec<_>>(),
            vec!["".to_string(), "foo.bar".to_string()]
        );
    }

    #[test]
    fn prost_resolver_uses_prost_naming_helpers() {
        let schema = SchemaSet::default();
        let prost = schema.prost();

        assert_eq!(prost.rust_service_name("FOO_BAR"), "FooBar");
        assert_eq!(prost.rust_method_name("XMLHttpRequest"), "xml_http_request");
        assert_eq!(prost.rust_package_file_stem("foo.FOO_BAR"), "foo.foo_bar");
    }
}
