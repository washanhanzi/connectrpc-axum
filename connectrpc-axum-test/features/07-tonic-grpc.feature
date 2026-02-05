Feature: connectrpc-axum-test integration behavior — tonic and gRPC
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
  # - connectrpc-axum-test/src/tonic_unary.rs (orchestrator)
  # - connectrpc-axum-test/src/tonic_unary/server.rs (Rust server: TonicCompatibleBuilder + MakeServiceBuilder)
  # - connectrpc-axum-test/src/tonic_unary/client.rs (Rust client: Connect JSON + gRPC protobuf)
  # - connectrpc-axum-test/go/tonic_unary/server/server.go (Go server: h2c for gRPC)
  # - connectrpc-axum-test/go/tonic_unary/client/client.go (Go client: connect-go + grpc-go)

  Scenario: tonic_unary — unary RPC works through Connect protocol
    Given a server with TonicCompatibleBuilder supporting both Connect and gRPC
    When the client sends a Connect unary JSON request
    Then the response contains the greeting

  Scenario: tonic_unary — unary RPC works through gRPC protocol
    Given a server with TonicCompatibleBuilder supporting both Connect and gRPC
    When the client sends a gRPC request over HTTP/2 with protobuf encoding
    Then the gRPC response contains the greeting


  # Source refs:
  # - connectrpc-axum-test/src/tonic_server_stream.rs (orchestrator)
  # - connectrpc-axum-test/src/tonic_server_stream/server.rs (Rust server: TonicCompatibleBuilder with streaming)
  # - connectrpc-axum-test/src/tonic_server_stream/client.rs (Rust client: Connect streaming + gRPC streaming)
  # - connectrpc-axum-test/go/tonic_server_stream/server/server.go (Go server: h2c)
  # - connectrpc-axum-test/go/tonic_server_stream/client/client.go (Go client)

  Scenario: tonic_server_stream — server streaming works through Connect protocol
    Given a server with TonicCompatibleBuilder for SayHelloStream
    When the client sends a Connect server stream request
    Then the response contains at least 2 streaming messages

  Scenario: tonic_server_stream — server streaming works through gRPC protocol
    Given a server with TonicCompatibleBuilder for SayHelloStream
    When the client sends a gRPC server stream request over HTTP/2
    Then the gRPC response contains at least 2 streamed messages


  # Source refs:
  # - connectrpc-axum-test/src/tonic_bidi_server.rs (orchestrator)
  # - connectrpc-axum-test/src/tonic_bidi_server/server.rs (Rust server: HelloWorldService + EchoService via tonic)
  # - connectrpc-axum-test/src/tonic_bidi_server/client.rs (Rust client: Connect unary + gRPC bidi + gRPC client stream)
  # - connectrpc-axum-test/go/tonic_bidi_server/server/server.go (Go server: h2c)
  # - connectrpc-axum-test/go/tonic_bidi_server/client/client.go (Go client)

  Scenario: tonic_bidi_server — Connect unary RPC works on a tonic bidi server
    Given a server with TonicCompatibleBuilder for HelloWorldService and EchoService
    When the client sends a Connect unary request
    Then the response contains the greeting

  Scenario: tonic_bidi_server — gRPC bidirectional streaming works on tonic
    Given a server with TonicCompatibleBuilder for EchoService (bidi)
    When the client opens a gRPC bidirectional stream over HTTP/2
    Then the stream exchanges messages with echo responses

  Scenario: tonic_bidi_server — gRPC client streaming works on tonic
    Given a server with TonicCompatibleBuilder for EchoService (client stream)
    When the client sends a gRPC client stream over HTTP/2
    Then the server returns the aggregated response


  # Source refs:
  # - connectrpc-axum-test/src/grpc_web.rs (orchestrator)
  # - connectrpc-axum-test/src/grpc_web/server.rs (Rust server: TonicCompatibleBuilder + tonic_web::GrpcWebLayer)
  # - connectrpc-axum-test/src/grpc_web/client.rs (Rust client: gRPC-Web over HTTP/1.1)
  # - connectrpc-axum-test/go/grpc_web/server/server.go (Go server)
  # - connectrpc-axum-test/go/grpc_web/client/client.go (Go client)

  Scenario: grpc_web — gRPC-Web protocol requests are accepted and processed
    Given a server with gRPC-Web support via GrpcWebLayer
    When the client sends a gRPC-Web request with Content-Type: application/grpc-web+proto
    Then the protobuf-encoded response is returned successfully over HTTP/1.1


