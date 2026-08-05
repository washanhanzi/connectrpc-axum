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
  # - connectrpc-axum-test/src/connect_server_stream/client.rs (Rust protocol client and generated-client interceptor cases)
  # - connectrpc-axum-test/go/connect_server_stream/server/server.go (Go server: streams 2 messages)
  # - connectrpc-axum-test/go/connect_server_stream/client/client.go (Go client: 1 test case)

  Scenario: connect_server_stream — server streaming returns multiple messages
    Given a valid SayHelloStream request with name "Stream Tester"
    When the client sends a Connect server stream request
    Then the response contains at least 2 envelope-framed messages
    And the first message contains "Hello"
    And the response content-type is application/connect+json

  Scenario: connect_server_stream — typed on_receive sees the transmitted request headers
    Given a generated Rust client compresses a SayHelloStream request with gzip
    When its typed on_receive interceptor receives a response message
    Then the stream context request headers include application/connect+json
    And the stream context request headers include Connect-Protocol-Version 1
    And the stream context request headers include Connect-Content-Encoding gzip


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
  # - connectrpc-axum-test/src/connect_bidi_stream/client.rs (Rust client: HTTP/1.1 rejection, HTTP/2 streaming, send-interceptor wake test)
  # - connectrpc-axum-test/go/connect_bidi_stream/server/server.go (Go server: h2c, requires HTTP/2 for bidi)
  # - connectrpc-axum-test/go/connect_bidi_stream/client/client.go (Go client: HTTP/2 h2c transport)

  Scenario: connect_bidi_stream — bidirectional streaming echoes messages
    Given an EchoBidiStream server that echoes each message
    When the client sends 3 messages via Connect bidi streaming over HTTP/2
    Then the response contains at least 3 echo responses
    And the first echo contains "Echo #1"

  Scenario: connect_bidi_stream — HTTP/1 is rejected before reading the request stream
    Given a Connect bidirectional streaming request over HTTP/1.1
    When the request body sends one message and remains open
    Then the server promptly responds with HTTP 505
    And the response includes "Connection: close"
    And the response body is empty

  Scenario: connect_bidi_stream — send interceptor error wakes pending receive
    Given a Rust Connect client opens an HTTP/2 bidirectional stream
    And the receive side is waiting for the next response
    When an on_send interceptor rejects the next request message
    Then the receive side returns that send interceptor error instead of timing out


  # Source refs:
  # - connectrpc-axum-test/src/timeout_rechunked_stream.rs (orchestrator, Rust server only)
  # - connectrpc-axum-test/src/timeout_rechunked_stream/server.rs (Rust server: re-chunking middleware between handler and ConnectLayer)
  # - connectrpc-axum-test/src/timeout_rechunked_stream/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/timeout_rechunked_stream/client/client.go (Go client: connect-go with context deadline)

  Scenario: timeout_rechunked_stream — armed timeout delivers re-chunked stream intact
    Given a SayHelloStream server whose middleware re-chunks the response body into 8-byte chunks
    And the first message contains bytes that mimic an EndStream envelope header at a chunk boundary
    When the client sends Connect-Timeout-Ms so the deadline body wrapper is armed
    Then the client receives all 3 messages and a non-error EndStream frame

  Scenario: timeout_rechunked_stream — no timeout delivers re-chunked stream intact
    Given a SayHelloStream server whose middleware re-chunks the response body into 8-byte chunks
    When the client sends no Connect-Timeout-Ms header
    Then the client receives all 3 messages and a non-error EndStream frame
