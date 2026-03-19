// Test that generated code can be included in a module without crate-level re-exports
// This demonstrates the fix for the issue where Tonic traits referenced types using
// crate::TypeName instead of super::TypeName

pub mod message_service {
    // Include the generated module tree in a nested module (not at crate root)
    include!(concat!(env!("OUT_DIR"), "/protos.rs"));

    // The generated tonic trait and sidecars should now correctly resolve types via
    // package-relative paths, so this module remains self-contained.
}

// Verify we can use the types from the module
#[allow(dead_code)]
fn test_types_accessible() {
    use message_service::hello::*;

    let _request = HelloRequest {
        name: Some("Test".to_string()),
        hobbies: vec![],
        greeting_type: None,
        ..Default::default()
    };

    // The tonic server trait should compile correctly
    // because it references super::HelloRequest (which exists in the same module)
    // instead of crate::HelloRequest (which would fail)
}
