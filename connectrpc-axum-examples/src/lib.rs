use std::net::SocketAddr;

// Generated protobuf types and services
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
    include!(concat!(env!("OUT_DIR"), "/echo.rs"));
}

// Re-export for convenience (not required for Tonic - it now uses super:: correctly)
pub use pb::*;

// Test module to verify the fix works without crate-level re-exports
mod test_module_include;

/// Returns the server address from PORT env var, defaulting to 3000.
///
/// This allows the integration test runner to assign unique ports to each server,
/// preventing port conflicts when tests run in parallel or when previous servers
/// haven't fully released the port.
///
/// # Example
///
/// ```ignore
/// let addr = connectrpc_axum_examples::server_addr();
/// let listener = tokio::net::TcpListener::bind(addr).await?;
/// ```
pub fn server_addr() -> SocketAddr {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    format!("0.0.0.0:{port}")
        .parse()
        .expect("invalid PORT env var")
}
