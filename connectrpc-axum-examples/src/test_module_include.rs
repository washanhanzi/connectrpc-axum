// Test that generated code can be included in a module without crate-level re-exports
// This demonstrates the fix for the issue where Tonic traits referenced types using
// crate::TypeName instead of super::TypeName

pub mod message_service {
    // Include generated code in a module (not at crate root)
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));

    // The generated tonic trait should now correctly reference types using super::TypeName
    // instead of crate::TypeName, so this module is self-contained
}

// Verify we can use the types from the module
#[allow(dead_code)]
fn test_types_accessible() {
    use message_service::*;

    let _request = HelloRequest {
        name: Some("Test".to_string()),
        hobbies: vec![],
        greeting_type: None,
    };

    // The tonic server trait should compile correctly
    // because it references super::HelloRequest (which exists in the same module)
    // instead of crate::HelloRequest (which would fail)
}
