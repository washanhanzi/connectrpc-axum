Feature: connectrpc-axum-test integration behavior — compression
  Plain-language BDD for current integration coverage in this domain.
  Each scenario is tested across all 4 client/server combinations:
    - Rust client → Rust server
    - Rust client → Go server
    - Go client → Rust server
    - Go client → Go server

  Background:
    Given a Rust test server and a Go test server are running on Unix sockets
    And both Rust and Go clients can call both servers

  # Source refs:
  # - connectrpc-axum-test/src/streaming_compression_gzip.rs (orchestrator)
  # - connectrpc-axum-test/src/streaming_compression_gzip/server.rs (Rust server: CompressionConfig with threshold)
  # - connectrpc-axum-test/src/streaming_compression_gzip/client.rs (Rust client: verifies compressed/uncompressed frames)
  # - connectrpc-axum-test/go/streaming_compression_gzip/server/server.go (Go server)
  # - connectrpc-axum-test/go/streaming_compression_gzip/client/client.go (Go client)

  Scenario: streaming_compression_gzip — server stream messages are compressed with gzip
    Given a SayHelloStream server with compression enabled (threshold-based)
    When the client sends Connect-Accept-Encoding: gzip
    Then the response Connect-Content-Encoding header is "gzip"
    And large frames have the compressed flag (0x01)
    And the compressed frames can be decompressed with gzip


  # Source refs:
  # - connectrpc-axum-test/src/client_streaming_compression.rs (orchestrator)
  # - connectrpc-axum-test/src/client_streaming_compression/server.rs (Rust server: EchoClientStream with compression)
  # - connectrpc-axum-test/src/client_streaming_compression/client.rs (Rust client: sends gzip-compressed frames)
  # - connectrpc-axum-test/go/client_streaming_compression/server/server.go (Go server)
  # - connectrpc-axum-test/go/client_streaming_compression/client/client.go (Go client)

  Scenario: client_streaming_compression — compressed client stream frames are decompressed
    Given an EchoClientStream server with compression support
    When the client sends gzip-compressed envelope frames with Connect-Content-Encoding: gzip
    Then the server decompresses and processes all messages correctly


  # Source refs:
  # - connectrpc-axum-test/src/compression_algos.rs (orchestrator, Rust server only)
  # - connectrpc-axum-test/src/compression_algos/server.rs (Rust server: compression-full support)
  # - connectrpc-axum-test/src/compression_algos/client.rs (Rust client: deflate, brotli, zstd tests)
  # - connectrpc-axum-test/go/compression_algos/client/client.go (Go client)
  # Note: Go server only supports gzip; tested against Rust server only

  Scenario: compression_algos — streaming responses compressed with deflate are decoded correctly
    Given a SayHelloStream server with full compression support
    When the client requests Connect-Accept-Encoding: deflate
    Then the compressed frames are valid deflate and can be decompressed

  Scenario: compression_algos — streaming responses compressed with brotli are decoded correctly
    Given a SayHelloStream server with full compression support
    When the client requests Connect-Accept-Encoding: br
    Then the compressed frames are valid brotli and can be decompressed

  Scenario: compression_algos — streaming responses compressed with zstd are decoded correctly
    Given a SayHelloStream server with full compression support
    When the client requests Connect-Accept-Encoding: zstd
    Then the compressed frames are valid zstd and can be decompressed

  Scenario: compression_algos — client streams compressed with deflate are decompressed
    Given an EchoClientStream server with full compression support
    When the client sends deflate-compressed frames with Connect-Content-Encoding: deflate
    Then the server decompresses and processes all messages correctly

  Scenario: compression_algos — client streams compressed with brotli are decompressed
    Given an EchoClientStream server with full compression support
    When the client sends brotli-compressed frames with Connect-Content-Encoding: br
    Then the server decompresses and processes all messages correctly

  Scenario: compression_algos — client streams compressed with zstd are decompressed
    Given an EchoClientStream server with full compression support
    When the client sends zstd-compressed frames with Connect-Content-Encoding: zstd
    Then the server decompresses and processes all messages correctly

  Scenario: compression_algos — unary gzip compression works end to end
    Given a SayHello server with full compression support
    When the client sends a gzip-compressed unary request and Accept-Encoding: gzip
    Then the response is gzip-compressed and contains the expected greeting

  Scenario: compression_algos — unary deflate compression works end to end
    Given a SayHello server with full compression support
    When the client sends a deflate-compressed unary request and Accept-Encoding: deflate
    Then the response is deflate-compressed and contains the expected greeting

  Scenario: compression_algos — unary brotli compression works end to end
    Given a SayHello server with full compression support
    When the client sends a brotli-compressed unary request and Accept-Encoding: br
    Then the response is brotli-compressed and contains the expected greeting

  Scenario: compression_algos — unary zstd compression works end to end
    Given a SayHello server with full compression support
    When the client sends a zstd-compressed unary request and Accept-Encoding: zstd
    Then the response is zstd-compressed and contains the expected greeting


