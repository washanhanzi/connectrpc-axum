use super::AxumConnectServiceGenerator;
use prost_build::{Method, Service, ServiceGenerator};

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
    let mut generator = AxumConnectServiceGenerator::with_tonic(false);
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
    let mut generator = AxumConnectServiceGenerator::with_tonic(true);
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
