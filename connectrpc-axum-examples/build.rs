fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate ConnectRPC service code
    // Uses default configuration with serde support for JSON serialization
    connectrpc_axum_build::compile_dir("proto")
        // Temporarily disable gRPC while we refine the implementation
        // .with_grpc() // Enable Tonic gRPC code generation with auto-adapter
        .compile()?;

    Ok(())
}
