fn main() -> Result<(), Box<dyn std::error::Error>> {
    let builder = connectrpc_axum_build::compile_dir("proto")
        .include_file("protos.rs")
        .with_connect_client();

    #[cfg(feature = "tonic")]
    let builder = builder.with_tonic();

    builder.compile()?;
    Ok(())
}
