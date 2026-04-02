use super::AxumConnectServiceGenerator;
use crate::schema::SchemaSet;
use prost_types::method_options::IdempotencyLevel;
use prost_types::{
    DescriptorProto, FileDescriptorProto, FileDescriptorSet, MethodDescriptorProto, MethodOptions,
    ServiceDescriptorProto,
};
use std::{collections::BTreeSet, fs};
use tempfile::tempdir;

fn render_service(
    package: &str,
    service_name: &str,
    methods: Vec<MethodDescriptorProto>,
    generator: AxumConnectServiceGenerator,
) -> String {
    let message_types = message_types_for_methods(&methods);
    let schema = SchemaSet::from_file_descriptor_set(&FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("test.proto".to_string()),
            package: if package.is_empty() {
                None
            } else {
                Some(package.to_string())
            },
            message_type: message_types,
            service: vec![ServiceDescriptorProto {
                name: Some(service_name.to_string()),
                method: methods,
                ..Default::default()
            }],
            ..Default::default()
        }],
    });

    generator
        .generate_service(&schema, &schema.services[0])
        .expect("service generation should succeed")
}

fn message_types_for_methods(methods: &[MethodDescriptorProto]) -> Vec<DescriptorProto> {
    let mut names = BTreeSet::new();
    for method in methods {
        if let Some(input_type) = method.input_type.as_deref() {
            if let Some(name) = input_type.rsplit('.').next()
                && !name.is_empty()
            {
                names.insert(name.to_string());
            }
        }
        if let Some(output_type) = method.output_type.as_deref() {
            if let Some(name) = output_type.rsplit('.').next()
                && !name.is_empty()
            {
                names.insert(name.to_string());
            }
        }
    }

    names
        .into_iter()
        .map(|name| DescriptorProto {
            name: Some(name),
            ..Default::default()
        })
        .collect()
}

fn method(
    package: &str,
    name: &str,
    input: &str,
    output: &str,
    client_streaming: bool,
    server_streaming: bool,
    options: MethodOptions,
) -> MethodDescriptorProto {
    let qualify = |ty: &str| {
        if package.is_empty() {
            format!(".{ty}")
        } else {
            format!(".{package}.{ty}")
        }
    };

    MethodDescriptorProto {
        name: Some(name.to_string()),
        input_type: Some(qualify(input)),
        output_type: Some(qualify(output)),
        client_streaming: Some(client_streaming),
        server_streaming: Some(server_streaming),
        options: Some(options),
        ..Default::default()
    }
}

#[test]
fn test_no_tonic_codegen() {
    let buf = render_service(
        "hello",
        "HelloWorldService",
        vec![method(
            "hello",
            "SayHello",
            "HelloRequest",
            "HelloResponse",
            false,
            false,
            Default::default(),
        )],
        AxumConnectServiceGenerator::new().with_connect_server(true),
    );

    assert!(!buf.contains("TonicCompatible"));
    assert!(!buf.contains("TonicService"));
    assert!(!buf.contains("tonic::"));
    assert!(!buf.contains("BoxedCall"));
    assert!(!buf.contains("_server::"));
    assert!(buf.contains("HelloWorldServiceBuilder"));
    assert!(buf.contains("say_hello"));
    assert!(buf.contains("ConnectHandlerWrapper"));
}

#[test]
fn test_with_tonic_codegen() {
    let buf = render_service(
        "hello",
        "HelloWorldService",
        vec![method(
            "hello",
            "SayHello",
            "HelloRequest",
            "HelloResponse",
            false,
            false,
            Default::default(),
        )],
        AxumConnectServiceGenerator::new()
            .with_connect_server(true)
            .with_tonic(true),
    );

    assert!(buf.contains("TonicCompatible"));
    assert!(buf.contains("HelloWorldTonicService"));
    assert!(buf.contains("BoxedCall"));
    assert!(buf.contains("HelloWorldServiceBuilder"));
}

#[test]
fn test_idempotency_level_no_side_effects() {
    let mut method_options = MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::NoSideEffects as i32);

    let buf = render_service(
        "user",
        "UserService",
        vec![method(
            "user",
            "GetUser",
            "GetUserRequest",
            "GetUserResponse",
            false,
            false,
            method_options,
        )],
        AxumConnectServiceGenerator::new().with_connect_server(true),
    );

    assert!(buf.contains("IdempotencyLevel :: NoSideEffects"));
}

#[test]
fn test_idempotency_level_idempotent() {
    let mut method_options = MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::Idempotent as i32);

    let buf = render_service(
        "user",
        "UserService",
        vec![method(
            "user",
            "DeleteUser",
            "DeleteUserRequest",
            "DeleteUserResponse",
            false,
            false,
            method_options,
        )],
        AxumConnectServiceGenerator::new().with_connect_server(true),
    );

    assert!(buf.contains("IdempotencyLevel :: Idempotent"));
}

#[test]
fn test_idempotency_level_unknown_default() {
    let buf = render_service(
        "user",
        "UserService",
        vec![method(
            "user",
            "CreateUser",
            "CreateUserRequest",
            "CreateUserResponse",
            false,
            false,
            Default::default(),
        )],
        AxumConnectServiceGenerator::new().with_connect_server(true),
    );

    assert!(buf.contains("IdempotencyLevel :: Unknown"));
    assert!(buf.contains("post_connect :: <"));
    assert!(!buf.contains("get_connect"));
}

#[test]
fn test_no_side_effects_enables_get_routing() {
    let mut method_options = MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::NoSideEffects as i32);

    let buf = render_service(
        "user",
        "UserService",
        vec![method(
            "user",
            "GetUser",
            "GetUserRequest",
            "GetUserResponse",
            false,
            false,
            method_options,
        )],
        AxumConnectServiceGenerator::new().with_connect_server(true),
    );

    assert!(buf.contains("get_connect"));
    assert!(buf.contains("post_connect"));
    assert!(buf.contains(". merge"));
    assert!(buf.contains("GET+POST enabled"));
}

#[test]
fn test_connect_codegen_enforces_exact_handler_signatures() {
    let buf = render_service(
        "hello",
        "HelloWorldService",
        vec![
            method(
                "hello",
                "SayHello",
                "HelloRequest",
                "HelloResponse",
                false,
                false,
                Default::default(),
            ),
            method(
                "hello",
                "SayHelloStream",
                "HelloRequest",
                "HelloResponse",
                false,
                true,
                Default::default(),
            ),
            method(
                "hello",
                "UploadHello",
                "HelloRequest",
                "HelloResponse",
                true,
                false,
                Default::default(),
            ),
            method(
                "hello",
                "ChatHello",
                "HelloRequest",
                "HelloResponse",
                true,
                true,
                Default::default(),
            ),
        ],
        AxumConnectServiceGenerator::new().with_connect_server(true),
    );

    assert!(
        !buf.contains("UnaryConnectHandler")
            && !buf.contains("ServerStreamConnectHandler")
            && !buf.contains("ClientStreamConnectHandler")
            && !buf.contains("BidiStreamConnectHandler"),
    );
    assert!(
        buf.matches(
            "ConnectHandlerWrapper < F , super :: HelloRequest , super :: HelloResponse , >"
        )
        .count()
            >= 4,
        "Generated:\n{}",
        buf
    );
}

#[test]
fn test_keyword_method_names_generate_valid_rust_identifiers() {
    let buf = render_service(
        "keyword",
        "Mover",
        vec![method(
            "keyword",
            "Move",
            "Empty",
            "Empty",
            false,
            false,
            Default::default(),
        )],
        AxumConnectServiceGenerator::new()
            .with_connect_server(true)
            .with_connect_client(true)
            .with_tonic(true),
    );

    assert!(buf.contains("pub fn r#move"));
    assert!(buf.contains("pub async fn r#move"));
    assert!(buf.contains("move_interceptors"));
    assert!(!buf.contains("r#move_interceptors"));
    assert!(buf.contains("pub const MOVE"));
}

#[test]
fn test_keyword_method_names_append_to_existing_output_file() {
    let message_types = vec![DescriptorProto {
        name: Some("Empty".to_string()),
        ..Default::default()
    }];
    let schema = SchemaSet::from_file_descriptor_set(&FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("keyword.proto".to_string()),
            package: Some("keyword".to_string()),
            message_type: message_types,
            service: vec![ServiceDescriptorProto {
                name: Some("Mover".to_string()),
                method: vec![method(
                    "keyword",
                    "Move",
                    "Empty",
                    "Empty",
                    false,
                    false,
                    Default::default(),
                )],
                ..Default::default()
            }],
            ..Default::default()
        }],
    });

    let out_dir = tempdir().expect("temp dir");
    let keyword_file = out_dir.path().join("keyword.rs");
    fs::write(&keyword_file, "// prost output\n").expect("write placeholder prost output");

    AxumConnectServiceGenerator::new()
        .with_connect_server(true)
        .with_connect_client(true)
        .with_tonic(true)
        .append_to_out_dir(&schema, out_dir.path().to_str().expect("utf-8 temp path"))
        .expect("append generated code");

    let generated = fs::read_to_string(&keyword_file).expect("read appended output");
    assert!(generated.starts_with("// prost output\n"));
    assert!(generated.contains("// --- Connect service/client code ---"));
    assert!(generated.contains("pub fn r#move"));
    assert!(generated.contains("pub async fn r#move"));
    assert!(generated.contains("pub const MOVE"));
}
