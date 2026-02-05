Feature: connectrpc-axum-test integration behavior — core unary
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
  # - connectrpc-axum-test/src/server_timeout.rs (orchestrator)
  # - connectrpc-axum-test/src/server_timeout/server.rs (Rust server: 500ms delay)
  # - connectrpc-axum-test/src/server_timeout/client.rs (Rust client: 3 test cases)
  # - connectrpc-axum-test/go/server_timeout/server/server.go (Go server: 500ms delay)
  # - connectrpc-axum-test/go/server_timeout/client/client.go (Go client: 3 test cases)

  Scenario: server_timeout — short timeout returns deadline_exceeded
    Given a valid unary SayHello request
    And the server sleeps 500ms before responding
    When the client sends Connect-Timeout-Ms set to 100
    Then the response code is deadline_exceeded

  Scenario: server_timeout — long timeout succeeds
    Given a valid unary SayHello request
    And the server sleeps 500ms before responding
    When the client sends Connect-Timeout-Ms set to 1000
    Then the response contains a non-empty message

  Scenario: server_timeout — no timeout header succeeds
    Given a valid unary SayHello request
    And the server sleeps 500ms before responding
    When the client sends no Connect-Timeout-Ms header
    Then the response contains a non-empty message


  # Source refs:
  # - connectrpc-axum-test/src/connect_unary.rs (orchestrator)
  # - connectrpc-axum-test/src/connect_unary/server.rs (Rust server)
  # - connectrpc-axum-test/src/connect_unary/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/connect_unary/server/server.go (Go server)
  # - connectrpc-axum-test/go/connect_unary/client/client.go (Go client: 2 test cases)

  Scenario: connect_unary — unary with name returns greeting
    Given a valid unary SayHello request with name "Alice"
    When the client sends a Connect unary request
    Then the response message is "Hello, Alice!"
    And the response content-type is application/json

  Scenario: connect_unary — unary with default name returns greeting
    Given a valid unary SayHello request with no name
    When the client sends a Connect unary request
    Then the response message is "Hello, World!"
    And the response content-type is application/json


  # Source refs:
  # - connectrpc-axum-test/src/protocol_version.rs (orchestrator)
  # - connectrpc-axum-test/src/protocol_version/server.rs (Rust server)
  # - connectrpc-axum-test/src/protocol_version/client.rs (Rust client: 3 test cases)
  # - connectrpc-axum-test/go/protocol_version/server/server.go (Go server)
  # - connectrpc-axum-test/go/protocol_version/client/client.go (Go client: 3 test cases)

  Scenario: protocol_version — valid protocol version succeeds
    Given a valid unary SayHello request
    When the client sends Connect-Protocol-Version set to "1"
    Then the response contains a non-empty message

  Scenario: protocol_version — missing protocol version is rejected
    Given a valid unary SayHello request
    When the client sends no Connect-Protocol-Version header
    Then the server returns an error response

  Scenario: protocol_version — invalid protocol version is rejected
    Given a valid unary SayHello request
    When the client sends Connect-Protocol-Version set to "2"
    Then the server returns an error response


  # Source refs:
  # - connectrpc-axum-test/src/get_request.rs (orchestrator)
  # - connectrpc-axum-test/src/get_request/server.rs (Rust server: GetGreeting handler)
  # - connectrpc-axum-test/src/get_request/client.rs (Rust client: 4 test cases)
  # - connectrpc-axum-test/go/get_request/server/server.go (Go server: GetGreeting handler)
  # - connectrpc-axum-test/go/get_request/client/client.go (Go client: 4 test cases)

  Scenario: get_request — GET with JSON encoding and name
    Given a GetGreeting method with idempotency_level NO_SIDE_EFFECTS
    When the client sends an HTTP GET with encoding=json, connect=v1, and URL-encoded message {"name":"Alice"}
    Then the response message is "Hello, Alice!"

  Scenario: get_request — GET with JSON encoding and default name
    Given a GetGreeting method with idempotency_level NO_SIDE_EFFECTS
    When the client sends an HTTP GET with encoding=json, connect=v1, and URL-encoded message {}
    Then the response message is "Hello, World!"

  Scenario: get_request — GET with base64-encoded JSON message
    Given a GetGreeting method with idempotency_level NO_SIDE_EFFECTS
    When the client sends an HTTP GET with encoding=json, connect=v1, base64=1, and base64url-encoded message
    Then the base64-encoded JSON is decoded and the response message is "Hello, Alice!"

  Scenario: get_request — GET missing message parameter
    Given a GetGreeting method with idempotency_level NO_SIDE_EFFECTS
    When the client sends an HTTP GET without the message query parameter
    Then the server returns an error response


  # Source refs:
  # - connectrpc-axum-test/src/protocol_negotiation.rs (orchestrator)
  # - connectrpc-axum-test/src/protocol_negotiation/server.rs (Rust server: standard Connect server)
  # - connectrpc-axum-test/src/protocol_negotiation/client.rs (Rust client: 3 test cases)
  # - connectrpc-axum-test/go/protocol_negotiation/server/server.go (Go server: standard connect-go server)
  # - connectrpc-axum-test/go/protocol_negotiation/client/client.go (Go client: 3 test cases)

  Scenario: protocol_negotiation — POST with text/plain returns HTTP 415
    Given a standard Connect server
    When the client sends a POST with Content-Type text/plain
    Then the HTTP status code is 415
    And the response body is empty

  Scenario: protocol_negotiation — POST with application/xml returns HTTP 415
    Given a standard Connect server
    When the client sends a POST with Content-Type application/xml
    Then the HTTP status code is 415
    And the response body is empty

  Scenario: protocol_negotiation — GET with unsupported encoding returns HTTP 415
    Given a standard Connect server with a GET-capable method
    When the client sends a GET with encoding=msgpack
    Then the HTTP status code is 415
    And the response body is empty


  # Source refs:
  # - connectrpc-axum-test/src/idempotency_get_connect_client.rs (orchestrator)
  # - connectrpc-axum-test/src/idempotency_get_connect_client/server.rs (Rust server: GetGreeting handler)
  # - connectrpc-axum-test/src/idempotency_get_connect_client/client.rs (Rust client: HTTP GET idempotent request)
  # - connectrpc-axum-test/go/idempotency_get_connect_client/server/server.go (Go server)
  # - connectrpc-axum-test/go/idempotency_get_connect_client/client/client.go (Go client: connect.WithHTTPGet())

  Scenario: idempotency_get_connect_client — connect-go client uses HTTP GET for idempotent methods
    Given a GetGreeting method with idempotency_level NO_SIDE_EFFECTS
    When the connect-go client sends a request with HTTPGet option
    Then the request is sent as HTTP GET and the response is returned successfully

