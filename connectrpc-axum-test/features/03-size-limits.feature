Feature: connectrpc-axum-test integration behavior — size limits
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
  # - connectrpc-axum-test/src/send_max_bytes.rs (orchestrator)
  # - connectrpc-axum-test/src/send_max_bytes/server.rs (Rust server: send_max_bytes=64)
  # - connectrpc-axum-test/src/send_max_bytes/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/send_max_bytes/server/server.go (Go server: SendMaxBytes=64)
  # - connectrpc-axum-test/go/send_max_bytes/client/client.go (Go client: 2 test cases)

  Scenario: send_max_bytes — small response succeeds under send limit
    Given a SayHello server with send_max_bytes set to 64
    When the client sends a request that produces a small response
    Then the response is returned successfully

  Scenario: send_max_bytes — large response fails when it exceeds send limit
    Given a SayHello server with send_max_bytes set to 64
    When the client sends a request that produces a response exceeding 64 bytes
    Then the server returns a resource_exhausted error


  # Source refs:
  # - connectrpc-axum-test/src/receive_max_bytes.rs (orchestrator)
  # - connectrpc-axum-test/src/receive_max_bytes/server.rs (Rust server: receive_max_bytes=64)
  # - connectrpc-axum-test/src/receive_max_bytes/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/receive_max_bytes/server/server.go (Go server: ReadMaxBytes=64)
  # - connectrpc-axum-test/go/receive_max_bytes/client/client.go (Go client: 2 test cases)

  Scenario: receive_max_bytes — small request succeeds under receive limit
    Given a SayHello server with receive_max_bytes set to 64
    When the client sends a small request body
    Then the response is returned successfully

  Scenario: receive_max_bytes — large request fails when it exceeds receive limit
    Given a SayHello server with receive_max_bytes set to 64
    When the client sends a request body exceeding 64 bytes
    Then the server returns a resource_exhausted error


  # Source refs:
  # - connectrpc-axum-test/src/streaming_send_max_bytes.rs (orchestrator)
  # - connectrpc-axum-test/src/streaming_send_max_bytes/server.rs (Rust server: SayHelloStream with send_max_bytes=64)
  # - connectrpc-axum-test/src/streaming_send_max_bytes/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/streaming_send_max_bytes/server/server.go (Go server: WithSendMaxBytes(64))
  # - connectrpc-axum-test/go/streaming_send_max_bytes/client/client.go (Go client: 2 test cases)

  Scenario: streaming_send_max_bytes — small streaming responses succeed under send limit
    Given a SayHelloStream server with send_max_bytes set to 64
    When the client sends a request that produces small stream messages
    Then the stream messages are returned successfully

  Scenario: streaming_send_max_bytes — large streaming response fails when it exceeds send limit
    Given a SayHelloStream server with send_max_bytes set to 64
    When the client sends a request that produces a stream message exceeding 64 bytes
    Then the EndStream frame contains a resource_exhausted error


  # Source refs:
  # - connectrpc-axum-test/src/streaming_receive_max_bytes.rs (orchestrator)
  # - connectrpc-axum-test/src/streaming_receive_max_bytes/server.rs (Rust server: SayHelloStream with receive_max_bytes=64)
  # - connectrpc-axum-test/src/streaming_receive_max_bytes/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/streaming_receive_max_bytes/server/server.go (Go server: WithReadMaxBytes(64))
  # - connectrpc-axum-test/go/streaming_receive_max_bytes/client/client.go (Go client: 2 test cases)

  Scenario: streaming_receive_max_bytes — small streaming request succeeds under receive limit
    Given a SayHelloStream server with receive_max_bytes set to 64
    When the client sends a small envelope-framed streaming request
    Then the stream messages are returned successfully

  Scenario: streaming_receive_max_bytes — large streaming request fails when it exceeds receive limit
    Given a SayHelloStream server with receive_max_bytes set to 64
    When the client sends an envelope-framed streaming request exceeding 64 bytes
    Then the server returns a resource_exhausted error


  # Source refs:
  # - connectrpc-axum-test/src/receive_max_bytes_5mb.rs (orchestrator)
  # - connectrpc-axum-test/src/receive_max_bytes_5mb/server.rs (Rust server: SayHello with receive_max_bytes=5MB)
  # - connectrpc-axum-test/src/receive_max_bytes_5mb/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/receive_max_bytes_5mb/server/server.go (Go server: WithReadMaxBytes(5*1024*1024))
  # - connectrpc-axum-test/go/receive_max_bytes_5mb/client/client.go (Go client: 2 test cases)

  Scenario: receive_max_bytes_5mb — small request succeeds under 5MB receive limit
    Given a SayHello server with receive_max_bytes set to 5MB
    When the client sends a small request body
    Then the response is returned successfully

  Scenario: receive_max_bytes_5mb — 6MB request fails with 5MB receive limit
    Given a SayHello server with receive_max_bytes set to 5MB
    When the client sends a request body exceeding 5MB
    Then the server returns a resource_exhausted error


  # Source refs:
  # - connectrpc-axum-test/src/receive_max_bytes_unlimited.rs (orchestrator)
  # - connectrpc-axum-test/src/receive_max_bytes_unlimited/server.rs (Rust server: SayHello with no receive_max_bytes)
  # - connectrpc-axum-test/src/receive_max_bytes_unlimited/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/receive_max_bytes_unlimited/server/server.go (Go server: no WithReadMaxBytes option)
  # - connectrpc-axum-test/go/receive_max_bytes_unlimited/client/client.go (Go client: 2 test cases)

  Scenario: receive_max_bytes_unlimited — 1MB request succeeds with unlimited receive
    Given a SayHello server with no receive_max_bytes limit
    When the client sends a 1MB request body
    Then the response is returned successfully

  Scenario: receive_max_bytes_unlimited — 2MB request succeeds with unlimited receive
    Given a SayHello server with no receive_max_bytes limit
    When the client sends a 2MB request body
    Then the response is returned successfully


