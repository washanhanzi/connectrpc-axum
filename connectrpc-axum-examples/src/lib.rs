// Re-export all generated types at crate root for Tonic's extern_path to find them
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
    include!(concat!(env!("OUT_DIR"), "/echo.rs"));
}

// Re-export all types at crate root so generated Tonic code can find them via crate::TypeName
pub use pb::*;
