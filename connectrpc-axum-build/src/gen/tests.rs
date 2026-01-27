use super::AxumConnectServiceGenerator;
use prost_build::{Method, Service, ServiceGenerator};
use prost_types::method_options::IdempotencyLevel;

#[test]
fn test_no_tonic_codegen() {
    // Create a mock service
    let service = Service {
        name: "HelloWorldService".to_string(),
        proto_name: "HelloWorldService".to_string(),
        package: "hello".to_string(),
        comments: Default::default(),
        methods: vec![Method {
            name: "SayHello".to_string(),
            proto_name: "SayHello".to_string(),
            comments: Default::default(),
            input_type: ".hello.HelloRequest".to_string(),
            output_type: ".hello.HelloResponse".to_string(),
            input_proto_type: "HelloRequest".to_string(),
            output_proto_type: "HelloResponse".to_string(),
            options: Default::default(),
            client_streaming: false,
            server_streaming: false,
        }],
        options: Default::default(),
    };

    // Generate WITHOUT tonic
    let mut generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    // Verify no tonic-related code is generated
    assert!(
        !buf.contains("TonicCompatible"),
        "Should not contain TonicCompatible when tonic is disabled"
    );
    assert!(
        !buf.contains("TonicService"),
        "Should not contain TonicService when tonic is disabled"
    );
    assert!(
        !buf.contains("tonic::"),
        "Should not contain tonic:: references when tonic is disabled"
    );
    assert!(
        !buf.contains("BoxedCall"),
        "Should not contain BoxedCall when tonic is disabled"
    );
    assert!(
        !buf.contains("_server::"),
        "Should not contain _server:: module references when tonic is disabled"
    );

    // Verify Connect-only code IS generated
    assert!(
        buf.contains("HelloWorldServiceBuilder"),
        "Should contain HelloWorldServiceBuilder"
    );
    assert!(buf.contains("say_hello"), "Should contain say_hello method");
    assert!(
        buf.contains("ConnectHandlerWrapper"),
        "Should contain ConnectHandlerWrapper for unary methods"
    );
}

#[test]
fn test_with_tonic_codegen() {
    // Create a mock service
    let service = Service {
        name: "HelloWorldService".to_string(),
        proto_name: "HelloWorldService".to_string(),
        package: "hello".to_string(),
        comments: Default::default(),
        methods: vec![Method {
            name: "SayHello".to_string(),
            proto_name: "SayHello".to_string(),
            comments: Default::default(),
            input_type: ".hello.HelloRequest".to_string(),
            output_type: ".hello.HelloResponse".to_string(),
            input_proto_type: "HelloRequest".to_string(),
            output_proto_type: "HelloResponse".to_string(),
            options: Default::default(),
            client_streaming: false,
            server_streaming: false,
        }],
        options: Default::default(),
    };

    // Generate WITH tonic
    let mut generator = AxumConnectServiceGenerator::new().with_connect_server(true).with_tonic(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    // Verify tonic-related code IS generated
    assert!(
        buf.contains("TonicCompatible"),
        "Should contain TonicCompatible when tonic is enabled"
    );
    assert!(
        buf.contains("HelloWorldTonicService"),
        "Should contain HelloWorldTonicService when tonic is enabled"
    );
    assert!(
        buf.contains("BoxedCall"),
        "Should contain BoxedCall when tonic is enabled"
    );

    // Verify Connect-only code is also generated
    assert!(
        buf.contains("HelloWorldServiceBuilder"),
        "Should contain HelloWorldServiceBuilder"
    );
}

#[test]
fn test_idempotency_level_no_side_effects() {
    // Create a mock service with a NO_SIDE_EFFECTS method
    let mut method_options = prost_types::MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::NoSideEffects as i32);

    let service = Service {
        name: "UserService".to_string(),
        proto_name: "UserService".to_string(),
        package: "user".to_string(),
        comments: Default::default(),
        methods: vec![Method {
            name: "GetUser".to_string(),
            proto_name: "GetUser".to_string(),
            comments: Default::default(),
            input_type: ".user.GetUserRequest".to_string(),
            output_type: ".user.GetUserResponse".to_string(),
            input_proto_type: "GetUserRequest".to_string(),
            output_proto_type: "GetUserResponse".to_string(),
            options: method_options,
            client_streaming: false,
            server_streaming: false,
        }],
        options: Default::default(),
    };

    let mut generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    // Verify idempotency level is present in generated code
    // Note: quote! outputs tokens with spaces around ::
    assert!(
        buf.contains("IdempotencyLevel :: NoSideEffects"),
        "Should contain IdempotencyLevel::NoSideEffects for NO_SIDE_EFFECTS method.\nGenerated:\n{}",
        buf
    );
}

#[test]
fn test_idempotency_level_idempotent() {
    // Create a mock service with an IDEMPOTENT method
    let mut method_options = prost_types::MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::Idempotent as i32);

    let service = Service {
        name: "UserService".to_string(),
        proto_name: "UserService".to_string(),
        package: "user".to_string(),
        comments: Default::default(),
        methods: vec![Method {
            name: "DeleteUser".to_string(),
            proto_name: "DeleteUser".to_string(),
            comments: Default::default(),
            input_type: ".user.DeleteUserRequest".to_string(),
            output_type: ".user.DeleteUserResponse".to_string(),
            input_proto_type: "DeleteUserRequest".to_string(),
            output_proto_type: "DeleteUserResponse".to_string(),
            options: method_options,
            client_streaming: false,
            server_streaming: false,
        }],
        options: Default::default(),
    };

    let mut generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    // Verify idempotency level is present in generated code
    // Note: quote! outputs tokens with spaces around ::
    assert!(
        buf.contains("IdempotencyLevel :: Idempotent"),
        "Should contain IdempotencyLevel::Idempotent for IDEMPOTENT method.\nGenerated:\n{}",
        buf
    );
}

#[test]
fn test_idempotency_level_unknown_default() {
    // Create a mock service without idempotency_level set (default)
    let service = Service {
        name: "UserService".to_string(),
        proto_name: "UserService".to_string(),
        package: "user".to_string(),
        comments: Default::default(),
        methods: vec![Method {
            name: "CreateUser".to_string(),
            proto_name: "CreateUser".to_string(),
            comments: Default::default(),
            input_type: ".user.CreateUserRequest".to_string(),
            output_type: ".user.CreateUserResponse".to_string(),
            input_proto_type: "CreateUserRequest".to_string(),
            output_proto_type: "CreateUserResponse".to_string(),
            options: Default::default(),
            client_streaming: false,
            server_streaming: false,
        }],
        options: Default::default(),
    };

    let mut generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    // Verify idempotency level defaults to Unknown
    // Note: quote! outputs tokens with spaces around ::
    assert!(
        buf.contains("IdempotencyLevel :: Unknown"),
        "Should contain IdempotencyLevel::Unknown for unset idempotency_level.\nGenerated:\n{}",
        buf
    );

    // Verify only POST is used (not GET) for methods without NoSideEffects
    assert!(
        buf.contains("post_connect (handler)"),
        "Should use post_connect only for non-idempotent methods.\nGenerated:\n{}",
        buf
    );
    assert!(
        !buf.contains("get_connect"),
        "Should NOT use get_connect for non-idempotent methods.\nGenerated:\n{}",
        buf
    );
}

#[test]
fn test_no_side_effects_enables_get_routing() {
    // Create a mock service with a NO_SIDE_EFFECTS method
    let mut method_options = prost_types::MethodOptions::default();
    method_options.idempotency_level = Some(IdempotencyLevel::NoSideEffects as i32);

    let service = Service {
        name: "UserService".to_string(),
        proto_name: "UserService".to_string(),
        package: "user".to_string(),
        comments: Default::default(),
        methods: vec![Method {
            name: "GetUser".to_string(),
            proto_name: "GetUser".to_string(),
            comments: Default::default(),
            input_type: ".user.GetUserRequest".to_string(),
            output_type: ".user.GetUserResponse".to_string(),
            input_proto_type: "GetUserRequest".to_string(),
            output_proto_type: "GetUserResponse".to_string(),
            options: method_options,
            client_streaming: false,
            server_streaming: false,
        }],
        options: Default::default(),
    };

    let mut generator = AxumConnectServiceGenerator::new().with_connect_server(true);
    let mut buf = String::new();
    generator.generate(service, &mut buf);

    // Verify GET routing is auto-enabled for NoSideEffects unary methods
    assert!(
        buf.contains("get_connect"),
        "Should auto-enable get_connect for NO_SIDE_EFFECTS unary methods.\nGenerated:\n{}",
        buf
    );
    assert!(
        buf.contains("post_connect"),
        "Should also include post_connect for NO_SIDE_EFFECTS unary methods.\nGenerated:\n{}",
        buf
    );
    // Verify both are merged together
    // Note: quote! outputs tokens with spaces around .
    assert!(
        buf.contains(". merge"),
        "Should merge get_connect and post_connect for NO_SIDE_EFFECTS unary methods.\nGenerated:\n{}",
        buf
    );
    // Verify the doc comment mentions GET+POST
    assert!(
        buf.contains("GET+POST enabled"),
        "Should document that GET+POST is enabled.\nGenerated:\n{}",
        buf
    );
}
