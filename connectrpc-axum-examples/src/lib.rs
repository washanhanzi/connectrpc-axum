use std::net::SocketAddr;

// Generated protobuf types and services
include!(concat!(env!("OUT_DIR"), "/protos.rs"));

// Re-export for convenience
pub use echo::*;
pub use hello::*;

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
