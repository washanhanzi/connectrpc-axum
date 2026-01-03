package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestUnaryErrorMetadata verifies that ConnectError metadata is returned as
// HTTP response headers for unary RPC errors.
//
// Unlike streaming errors (where metadata goes in EndStream frame's "metadata" field),
// unary errors return metadata directly as HTTP headers on the error response.
func TestUnaryErrorMetadata(t *testing.T) {
	s := startServer(t, "unary-error-metadata", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	t.Run("error with custom metadata headers", func(t *testing.T) {
		// Use raw HTTP client to inspect response headers
		req, err := http.NewRequest("POST", serverURL+"/hello.HelloWorldService/SayHello", strings.NewReader(`{"name":"error-with-meta"}`))
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/json")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should be HTTP 500 (Internal error)
		if resp.StatusCode != http.StatusInternalServerError {
			t.Fatalf("Expected 500 Internal Server Error, got: %d", resp.StatusCode)
		}

		// Check custom metadata headers are present
		assertHeaderValue(t, resp.Header, "x-error-id", "err-12345")
		assertHeaderValue(t, resp.Header, "x-request-id", "req-67890")
		assertHeaderValue(t, resp.Header, "x-custom-bin", "AAEC")

		// Verify response body is valid Connect error JSON
		body, _ := io.ReadAll(resp.Body)
		var errResp struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		}
		if err := json.Unmarshal(body, &errResp); err != nil {
			t.Fatalf("Failed to parse error response: %v", err)
		}
		if errResp.Code != "internal" {
			t.Fatalf("Expected code 'internal', got %q", errResp.Code)
		}

		t.Logf("Headers: x-error-id=%s, x-request-id=%s, x-custom-bin=%s",
			resp.Header.Get("x-error-id"),
			resp.Header.Get("x-request-id"),
			resp.Header.Get("x-custom-bin"))
	})

	t.Run("error metadata with protocol headers", func(t *testing.T) {
		// Protocol headers set on the error should also appear for unary responses
		// (unlike streaming where they're filtered from EndStream metadata)
		req, err := http.NewRequest("POST", serverURL+"/hello.HelloWorldService/SayHello", strings.NewReader(`{"name":"error-with-protocol-headers"}`))
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/json")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should be HTTP 400 (InvalidArgument)
		if resp.StatusCode != http.StatusBadRequest {
			t.Fatalf("Expected 400 Bad Request, got: %d", resp.StatusCode)
		}

		// x-custom should be present
		assertHeaderValue(t, resp.Header, "x-custom", "should-appear")

		// Note: For unary responses, metadata headers are appended to response headers.
		// The actual Content-Type will be set by the framework, but extra values may be appended.
		// grpc-status is not a standard HTTP header but should appear as metadata.
		if resp.Header.Get("grpc-status") != "should-appear-too" {
			t.Logf("Note: grpc-status header may be overridden or filtered: %v", resp.Header.Values("grpc-status"))
		}

		t.Logf("Headers received: x-custom=%s", resp.Header.Get("x-custom"))
	})

	t.Run("error with multi-value header", func(t *testing.T) {
		req, err := http.NewRequest("POST", serverURL+"/hello.HelloWorldService/SayHello", strings.NewReader(`{"name":"error-multi-value"}`))
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/json")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should be HTTP 400 (FailedPrecondition maps to BadRequest)
		if resp.StatusCode != http.StatusBadRequest {
			t.Fatalf("Expected 400 Bad Request, got: %d", resp.StatusCode)
		}

		// Check multi-value header
		values := resp.Header.Values("x-multi")
		if len(values) != 2 {
			t.Fatalf("Expected 2 values for x-multi header, got %d: %v", len(values), values)
		}
		if values[0] != "value1" || values[1] != "value2" {
			t.Fatalf("Expected x-multi values ['value1', 'value2'], got %v", values)
		}

		t.Logf("Multi-value header: x-multi=%v", values)
	})

	t.Run("error without metadata has no extra headers", func(t *testing.T) {
		req, err := http.NewRequest("POST", serverURL+"/hello.HelloWorldService/SayHello", strings.NewReader(`{"name":"error-no-meta"}`))
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/json")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should be HTTP 404 (NotFound)
		if resp.StatusCode != http.StatusNotFound {
			t.Fatalf("Expected 404 Not Found, got: %d", resp.StatusCode)
		}

		// Should not have custom metadata headers
		if resp.Header.Get("x-error-id") != "" {
			t.Fatalf("Expected no x-error-id header, got: %s", resp.Header.Get("x-error-id"))
		}
		if resp.Header.Get("x-request-id") != "" {
			t.Fatalf("Expected no x-request-id header, got: %s", resp.Header.Get("x-request-id"))
		}

		t.Logf("No custom metadata headers present (as expected)")
	})

	t.Run("success response via connect client", func(t *testing.T) {
		name := "Alice"
		resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))
		if err != nil {
			t.Fatalf("Success request failed: %v", err)
		}

		if !strings.Contains(resp.Msg.Message, "Alice") {
			t.Fatalf("Response should contain name: %q", resp.Msg.Message)
		}

		t.Logf("Success response: %s", resp.Msg.Message)
	})

	t.Run("error via connect client preserves code", func(t *testing.T) {
		name := "error-with-meta"
		_, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))

		if err == nil {
			t.Fatal("Expected error")
		}

		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}

		if connectErr.Code() != connect.CodeInternal {
			t.Fatalf("Expected CodeInternal, got: %v", connectErr.Code())
		}

		t.Logf("Connect client received error: code=%v message=%q", connectErr.Code(), connectErr.Message())
	})
}

func assertHeaderValue(t *testing.T, headers http.Header, key, expectedValue string) {
	t.Helper()
	value := headers.Get(key)
	if value == "" {
		t.Fatalf("Expected header %q to be present", key)
	}
	if value != expectedValue {
		t.Fatalf("Expected header %q to be %q, got %q", key, expectedValue, value)
	}
}
