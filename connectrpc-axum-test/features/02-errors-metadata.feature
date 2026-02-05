Feature: connectrpc-axum-test integration behavior — errors and metadata
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
  # - connectrpc-axum-test/src/error_details.rs (orchestrator)
  # - connectrpc-axum-test/src/error_details/server.rs (Rust server: always returns error with details)
  # - connectrpc-axum-test/src/error_details/client.rs (Rust client: 1 test case)
  # - connectrpc-axum-test/go/error_details/server/server.go (Go server: always returns error with details)
  # - connectrpc-axum-test/go/error_details/client/client.go (Go client: 1 test case)

  Scenario: error_details — error response includes structured details
    Given a SayHello request to a server that always returns an error
    When the client sends a Connect unary request
    Then the response code is "invalid_argument"
    And the response message is "name is required"
    And the response includes a non-empty details array


  # Source refs:
  # - connectrpc-axum-test/src/streaming_error.rs (orchestrator)
  # - connectrpc-axum-test/src/streaming_error/server.rs (Rust server: streaming handler returns immediate error)
  # - connectrpc-axum-test/src/streaming_error/client.rs (Rust client: 1 test case)
  # - connectrpc-axum-test/go/streaming_error/server/server.go (Go server: streaming handler returns immediate error)
  # - connectrpc-axum-test/go/streaming_error/client/client.go (Go client: 1 test case)

  Scenario: streaming_error — streaming error returned in EndStream frame
    Given a SayHelloStream request to a server that always returns an error
    When the client sends a Connect server stream request
    Then the HTTP status code is 200
    And the response contains an EndStream frame with error code "internal"
    And the EndStream error message is "something went wrong"


  # Source refs:
  # - connectrpc-axum-test/src/unary_error_metadata.rs (orchestrator)
  # - connectrpc-axum-test/src/unary_error_metadata/server.rs (Rust server: error with metadata headers)
  # - connectrpc-axum-test/src/unary_error_metadata/client.rs (Rust client: 1 test case)
  # - connectrpc-axum-test/go/unary_error_metadata/server/server.go (Go server: error with metadata headers)
  # - connectrpc-axum-test/go/unary_error_metadata/client/client.go (Go client: 1 test case)

  Scenario: unary_error_metadata — error response includes custom metadata headers
    Given a SayHello server that returns an error with metadata headers x-custom-meta and x-request-id
    When the client sends a unary request with no name
    Then the response error code is "invalid_argument"
    And the response error message is "name is required"
    And the response includes header x-custom-meta with value "custom-value"
    And the response includes header x-request-id with value "test-123"


  # Source refs:
  # - connectrpc-axum-test/src/endstream_metadata.rs (orchestrator)
  # - connectrpc-axum-test/src/endstream_metadata/server.rs (Rust server: streaming error with metadata)
  # - connectrpc-axum-test/src/endstream_metadata/client.rs (Rust client: 1 test case)
  # - connectrpc-axum-test/go/endstream_metadata/server/server.go (Go server: streaming error with metadata)
  # - connectrpc-axum-test/go/endstream_metadata/client/client.go (Go client: 1 test case)

  Scenario: endstream_metadata — streaming errors include metadata in EndStream frames
    Given a SayHelloStream server that returns an error with metadata headers x-custom-meta and x-request-id
    When the client sends a Connect server stream request
    Then the HTTP status code is 200
    And the response contains an EndStream frame
    And the EndStream frame error code is "internal"
    And the EndStream frame metadata includes x-custom-meta with value "custom-value"
    And the EndStream frame metadata includes x-request-id with value "req-123"


