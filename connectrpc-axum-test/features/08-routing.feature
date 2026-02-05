Feature: connectrpc-axum-test integration behavior — routing
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
  # - connectrpc-axum-test/src/axum_router.rs (orchestrator)
  # - connectrpc-axum-test/src/axum_router/server.rs (Rust server: plain axum routes + Connect RPC via MakeServiceBuilder)
  # - connectrpc-axum-test/src/axum_router/client.rs (Rust client: 3 test cases)
  # - connectrpc-axum-test/go/axum_router/server/server.go (Go server: http.ServeMux with plain handlers + connect handler)
  # - connectrpc-axum-test/go/axum_router/client/client.go (Go client: 3 test cases)

  Scenario: axum_router — health endpoint returns ok
    Given a server with plain axum routes for /health and /metrics alongside Connect RPC
    When the client sends GET /health
    Then the response body is "ok"

  Scenario: axum_router — metrics endpoint returns data
    Given a server with plain axum routes for /health and /metrics alongside Connect RPC
    When the client sends GET /metrics
    Then the response body is non-empty

  Scenario: axum_router — Connect RPC works alongside plain routes
    Given a server with plain axum routes for /health and /metrics alongside Connect RPC
    When the client sends a Connect unary SayHello request
    Then the response message contains the greeting


