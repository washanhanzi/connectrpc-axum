Feature: connectrpc-axum-test integration behavior — auth extractors
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
  # - connectrpc-axum-test/src/extractor_connect_error.rs (orchestrator)
  # - connectrpc-axum-test/src/extractor_connect_error/server.rs (Rust server: custom UserId extractor with ConnectError rejection)
  # - connectrpc-axum-test/src/extractor_connect_error/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/extractor_connect_error/server/server.go (Go server: header check returning connect.CodeUnauthenticated)
  # - connectrpc-axum-test/go/extractor_connect_error/client/client.go (Go client: 2 test cases)

  Scenario: extractor_connect_error — missing auth header returns UNAUTHENTICATED Connect error
    Given a SayHello server with a custom extractor that requires x-user-id header
    And the extractor rejects with ConnectError(Code::Unauthenticated)
    When the client sends a request without the x-user-id header
    Then the response error code is "unauthenticated"

  Scenario: extractor_connect_error — valid auth header succeeds
    Given a SayHello server with a custom extractor that requires x-user-id header
    When the client sends a request with x-user-id header set to "user-42"
    Then the response message contains the user ID


  # Source refs:
  # - connectrpc-axum-test/src/extractor_http_response.rs (orchestrator)
  # - connectrpc-axum-test/src/extractor_http_response/server.rs (Rust server: custom UserId extractor with plain HTTP 401 rejection)
  # - connectrpc-axum-test/src/extractor_http_response/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/extractor_http_response/server/server.go (Go server: middleware returning HTTP 401)
  # - connectrpc-axum-test/go/extractor_http_response/client/client.go (Go client: 2 test cases)

  Scenario: extractor_http_response — missing auth header returns plain HTTP 401
    Given a SayHello server with a custom extractor that requires x-user-id header
    And the extractor rejects with a plain HTTP 401 Unauthorized response
    When the client sends a request without the x-user-id header
    Then the HTTP status code is 401

  Scenario: extractor_http_response — valid auth header succeeds
    Given a SayHello server with a custom extractor that requires x-user-id header
    When the client sends a request with x-user-id header set to "user-42"
    Then the HTTP status code is 200
    And the response message contains the user ID


  # Source refs:
  # - connectrpc-axum-test/src/streaming_extractor.rs (orchestrator)
  # - connectrpc-axum-test/src/streaming_extractor/server.rs (Rust server: custom ApiKey extractor on SayHelloStream)
  # - connectrpc-axum-test/src/streaming_extractor/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/streaming_extractor/server/server.go (Go server: x-api-key check in handler)
  # - connectrpc-axum-test/go/streaming_extractor/client/client.go (Go client: 2 test cases)

  Scenario: streaming_extractor — missing auth header returns UNAUTHENTICATED on server stream
    Given a SayHelloStream server with a custom extractor requiring x-api-key header
    When the client sends a server stream request without the x-api-key header
    Then the EndStream frame contains an unauthenticated error

  Scenario: streaming_extractor — valid auth header returns stream messages
    Given a SayHelloStream server with a custom extractor requiring x-api-key header
    When the client sends a server stream request with x-api-key header
    Then the response contains at least 2 stream messages


  # Source refs:
  # - connectrpc-axum-test/src/streaming_extractor_client.rs (orchestrator)
  # - connectrpc-axum-test/src/streaming_extractor_client/server.rs (Rust server: ApiKey extractor on EchoClientStream)
  # - connectrpc-axum-test/src/streaming_extractor_client/client.rs (Rust client: 2 test cases)
  # - connectrpc-axum-test/go/streaming_extractor_client/server/server.go (Go server)
  # - connectrpc-axum-test/go/streaming_extractor_client/client/client.go (Go client)

  Scenario: streaming_extractor_client — client streaming without auth returns UNAUTHENTICATED
    Given an EchoClientStream server with a custom extractor requiring x-api-key header
    When the client sends a client stream request without the x-api-key header
    Then the response error code is "unauthenticated"

  Scenario: streaming_extractor_client — client streaming with auth succeeds
    Given an EchoClientStream server with a custom extractor requiring x-api-key header
    When the client sends a client stream request with x-api-key header
    Then the response message contains all sent messages


  # Source refs:
  # - connectrpc-axum-test/src/tonic_extractor.rs (orchestrator)
  # - connectrpc-axum-test/src/tonic_extractor/server.rs (Rust server: TonicCompatibleBuilder with ApiKey extractor)
  # - connectrpc-axum-test/src/tonic_extractor/client.rs (Rust client: Connect +/- key, gRPC +/- key)
  # - connectrpc-axum-test/go/tonic_extractor/server/server.go (Go server)
  # - connectrpc-axum-test/go/tonic_extractor/client/client.go (Go client)

  Scenario: tonic_extractor — Connect without key returns UNAUTHENTICATED
    Given a server with TonicCompatibleBuilder and a custom ApiKey extractor
    When the client sends a Connect request without x-api-key header
    Then the response error code is "unauthenticated"

  Scenario: tonic_extractor — Connect with key succeeds
    Given a server with TonicCompatibleBuilder and a custom ApiKey extractor
    When the client sends a Connect request with x-api-key header
    Then the response message contains the greeting and key

  Scenario: tonic_extractor — gRPC without key returns UNAUTHENTICATED
    Given a server with TonicCompatibleBuilder and a custom ApiKey extractor
    When the client sends a gRPC request without x-api-key header
    Then the gRPC status is UNAUTHENTICATED

  Scenario: tonic_extractor — gRPC with key succeeds
    Given a server with TonicCompatibleBuilder and a custom ApiKey extractor
    When the client sends a gRPC request with x-api-key header
    Then the gRPC response message contains the greeting and key


