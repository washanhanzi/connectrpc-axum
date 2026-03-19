use super::AxumConnectServiceGenerator;
use crate::TonicRequestMode;
use crate::model::{ProtoComments, ProtoMethod, ProtoService, ProtoTypeRef};
use convert_case::{Case, Casing};
use prost_types::method_options::IdempotencyLevel;

fn proto_service(
    package: &str,
    service_name: &str,
    method_name: &str,
    request_type: &str,
    response_type: &str,
    method_options: prost_types::MethodOptions,
) -> ProtoService {
    ProtoService {
        name: service_name.to_string(),
        proto_name: service_name.to_string(),
        package: package.to_string(),
        comments: ProtoComments::default(),
        methods: vec![ProtoMethod {
            name: method_name.to_case(Case::Snake),
            proto_name: method_name.to_string(),
            comments: ProtoComments::default(),
            input_type: ProtoTypeRef {
                proto_path: format!(".{package}.{request_type}"),
                rust_path: format!("{request_type}"),
            },
            output_type: ProtoTypeRef {
                proto_path: format!(".{package}.{response_type}"),
                rust_path: format!("{response_type}"),
            },
            input_proto_type: request_type.to_string(),
            output_proto_type: response_type.to_string(),
            options: method_options,
            client_streaming: false,
            server_streaming: false,
            deprecated: false,
        }],
        options: prost_types::ServiceOptions::default(),
        deprecated: false,
    }
}

#[test]
fn test_no_tonic_codegen() {
    let service = proto_service(
        "hello",
        "HelloWorldService",
        "SayHello",
        "HelloRequest",
        "HelloResponse",
        Default::default(),
    );

    let generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

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
    let service = proto_service(
        "hello",
        "HelloWorldService",
        "SayHello",
        "HelloRequest",
        "HelloResponse",
        Default::default(),
    );

    let generator = AxumConnectServiceGenerator::new()
        .with_connect_server(true)
        .with_tonic(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    assert!(buf.contains("TonicCompatible"));
    assert!(buf.contains("HelloWorldTonicService"));
    assert!(buf.contains("BoxedCall"));
    assert!(buf.contains("HelloWorldServiceBuilder"));
}

#[test]
fn test_with_tonic_view_codegen() {
    let service = proto_service(
        "hello",
        "HelloWorldService",
        "SayHello",
        "HelloRequest",
        "HelloResponse",
        Default::default(),
    );

    let generator = AxumConnectServiceGenerator::new()
        .with_connect_server(true)
        .with_tonic(true)
        .with_tonic_request_mode(TonicRequestMode::View);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    assert!(buf.contains("connectrpc_axum :: View < HelloRequest >"));
}

#[test]
fn test_idempotency_level_no_side_effects() {
    let mut method_options = prost_types::MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::NoSideEffects as i32);
    let service = proto_service(
        "user",
        "UserService",
        "GetUser",
        "GetUserRequest",
        "GetUserResponse",
        method_options,
    );

    let generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    assert!(buf.contains("IdempotencyLevel :: NoSideEffects"));
}

#[test]
fn test_idempotency_level_idempotent() {
    let mut method_options = prost_types::MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::Idempotent as i32);
    let service = proto_service(
        "user",
        "UserService",
        "DeleteUser",
        "DeleteUserRequest",
        "DeleteUserResponse",
        method_options,
    );

    let generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    assert!(buf.contains("IdempotencyLevel :: Idempotent"));
}

#[test]
fn test_idempotency_level_unknown_default() {
    let service = proto_service(
        "user",
        "UserService",
        "CreateUser",
        "CreateUserRequest",
        "CreateUserResponse",
        Default::default(),
    );

    let generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    assert!(buf.contains("IdempotencyLevel :: Unknown"));
    assert!(buf.contains("post_connect (handler)"));
    assert!(!buf.contains("get_connect"));
}

#[test]
fn test_no_side_effects_enables_get_routing() {
    let mut method_options = prost_types::MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::NoSideEffects as i32);
    let service = proto_service(
        "user",
        "UserService",
        "GetUser",
        "GetUserRequest",
        "GetUserResponse",
        method_options,
    );

    let generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    assert!(buf.contains("get_connect"));
    assert!(buf.contains("post_connect"));
    assert!(buf.contains(". merge"));
    assert!(buf.contains("GET+POST enabled"));
}
