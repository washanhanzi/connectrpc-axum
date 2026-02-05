Feature: connectrpc-axum-test integration behavior — streaming core
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
  # - connectrpc-axum-test/src/connect_server_stream.rs (orchestrator)
  # - connectrpc-axum-test/src/connect_server_stream/server.rs (Rust server: streams 2 messages)
  # - connectrpc-axum-test/src/connect_server_stream/client.rs (Rust client: 1 test case)
  # - connectrpc-axum-test/go/connect_server_stream/server/server.go (Go server: streams 2 messages)
  # - connectrpc-axum-test/go/connect_server_stream/client/client.go (Go client: 1 test case)

  Scenario: connect_server_stream — server streaming returns multiple messages
    Given a valid SayHelloStream request with name "Stream Tester"
    When the client sends a Connect server stream request
    Then the response contains at least 2 envelope-framed messages
    And the first message contains "Hello"
    And the response content-type is application/connect+json


  # Source refs:
  # - connectrpc-axum-test/src/connect_client_stream.rs (orchestrator)
  # - connectrpc-axum-test/src/connect_client_stream/server.rs (Rust server: EchoClientStream handler)
  # - connectrpc-axum-test/src/connect_client_stream/client.rs (Rust client: 1 test case)
  # - connectrpc-axum-test/go/connect_client_stream/server/server.go (Go server)
  # - connectrpc-axum-test/go/connect_client_stream/client/client.go (Go client)

  Scenario: connect_client_stream — client streaming aggregates messages
    Given an EchoClientStream server that collects all client messages
    When the client sends 3 envelope-framed messages and an EndStream frame
    Then the response contains all 3 message contents


  # Source refs:
  # - connectrpc-axum-test/src/connect_bidi_stream.rs (orchestrator)
  # - connectrpc-axum-test/src/connect_bidi_stream/server.rs (Rust server: EchoBidiStream handler)
  # - connectrpc-axum-test/src/connect_bidi_stream/client.rs (Rust client: HTTP/1.1 half-duplex, HTTP/2 for Go server)
  # - connectrpc-axum-test/go/connect_bidi_stream/server/server.go (Go server: h2c, requires HTTP/2 for bidi)
  # - connectrpc-axum-test/go/connect_bidi_stream/client/client.go (Go client: HTTP/2 h2c transport)

  Scenario: connect_bidi_stream — bidirectional streaming echoes messages
    Given an EchoBidiStream server that echoes each message
    When the client sends 3 messages via Connect bidi streaming
    Then the response contains at least 3 echo responses
    And the first echo contains "Echo #1"


