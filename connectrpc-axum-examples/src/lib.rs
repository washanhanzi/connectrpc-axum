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

/// Build a `curl` command for a Connect streaming request body.
///
/// Connect streaming requests use a 5-byte envelope before the JSON payload, so
/// raw `-d '{"..."}'` requests are invalid for `application/connect+json`.
pub fn connect_streaming_curl_command(path: &str, payload_json: &str) -> String {
    let mut framed_payload = String::new();

    framed_payload.push_str(r"\x00");
    for byte in (payload_json.len() as u32).to_be_bytes() {
        framed_payload.push_str(&format!(r"\x{byte:02x}"));
    }

    for ch in payload_json.chars() {
        match ch {
            '\\' => framed_payload.push_str(r"\\"),
            '\'' => framed_payload.push_str(r"'\''"),
            c if c.is_ascii() && !c.is_ascii_control() => framed_payload.push(c),
            c => {
                let mut buf = [0; 4];
                for byte in c.encode_utf8(&mut buf).as_bytes() {
                    framed_payload.push_str(&format!(r"\x{byte:02x}"));
                }
            }
        }
    }

    format!(
        "printf '%b' '{framed_payload}' | \\\n  curl -X POST http://localhost:3000{path} \\\n    -H 'Content-Type: application/connect+json' \\\n    --data-binary @-"
    )
}

#[cfg(test)]
mod tests {
    use super::connect_streaming_curl_command;

    #[test]
    fn streaming_curl_command_prefixes_the_connect_envelope() {
        let command = connect_streaming_curl_command(
            "/hello.HelloWorldService/SayHelloStream",
            r#"{"name": "Alice", "hobbies": ["coding", "reading"]}"#,
        );

        assert_eq!(
            command,
            "printf '%b' '\\x00\\x00\\x00\\x00\\x33{\"name\": \"Alice\", \"hobbies\": [\"coding\", \"reading\"]}' | \\\n  curl -X POST http://localhost:3000/hello.HelloWorldService/SayHelloStream \\\n    -H 'Content-Type: application/connect+json' \\\n    --data-binary @-"
        );
    }
}
