// Generated protobuf types and services
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
    include!(concat!(env!("OUT_DIR"), "/echo.rs"));
}

// Re-export for convenience (not required for Tonic - it now uses super:: correctly)
pub use pb::*;

// Test module to verify the fix works without crate-level re-exports
mod test_module_include;
