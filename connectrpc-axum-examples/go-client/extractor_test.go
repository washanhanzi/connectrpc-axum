package main

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestExtractorConnectError tests that extractor rejection with ConnectError
// is properly encoded using the Connect protocol.
// Tests handle_extractor_rejection (handler.rs:25) with ConnectError path.
func TestExtractorConnectError(t *testing.T) {
	s := startServer(t, "extractor-connect-error", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	t.Run("without header should fail with UNAUTHENTICATED", func(t *testing.T) {
		name := "Alice"
		_, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))

		if err == nil {
			t.Fatal("Expected error when x-user-id header is missing")
		}

		// Should be a Connect error with UNAUTHENTICATED code
		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}

		if connectErr.Code() != connect.CodeUnauthenticated {
			t.Fatalf("Expected CodeUnauthenticated, got: %v", connectErr.Code())
		}

		t.Logf("Got expected Connect error: code=%v message=%q", connectErr.Code(), connectErr.Message())
	})

	t.Run("with header should succeed", func(t *testing.T) {
		name := "Alice"
		req := connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		})
		req.Header().Set("x-user-id", "user123")

		resp, err := client.SayHello(context.Background(), req)
		if err != nil {
			t.Fatalf("Request with header failed: %v", err)
		}

		if !strings.Contains(resp.Msg.Message, "Alice") {
			t.Fatalf("Response should contain name: %q", resp.Msg.Message)
		}
		if !strings.Contains(resp.Msg.Message, "user123") {
			t.Fatalf("Response should contain user ID: %q", resp.Msg.Message)
		}

		t.Logf("Response: %s", resp.Msg.Message)
	})
}

// TestExtractorHTTPResponse tests that extractor rejection with plain HTTP response
// bypasses Connect protocol encoding and returns raw HTTP.
// Tests handle_extractor_rejection (handler.rs:25) with non-ConnectError path.
func TestExtractorHTTPResponse(t *testing.T) {
	s := startServer(t, "extractor-http-response", "")
	defer s.stop()

	t.Run("without header should return plain HTTP 401", func(t *testing.T) {
		// Use raw HTTP client to see the actual response
		req, err := http.NewRequest("POST", serverURL+"/hello.HelloWorldService/SayHello", strings.NewReader(`{"name":"Alice"}`))
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/json")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should be plain HTTP 401, not Connect-encoded error
		if resp.StatusCode != http.StatusUnauthorized {
			t.Fatalf("Expected 401 Unauthorized, got: %d", resp.StatusCode)
		}

		// Check for WWW-Authenticate header (plain HTTP auth challenge)
		wwwAuth := resp.Header.Get("WWW-Authenticate")
		if wwwAuth == "" {
			t.Fatal("Expected WWW-Authenticate header in plain HTTP 401 response")
		}

		body, _ := io.ReadAll(resp.Body)
		t.Logf("Got expected HTTP 401: WWW-Authenticate=%q body=%q", wwwAuth, string(body))
	})

	t.Run("with header should succeed", func(t *testing.T) {
		client := genconnect.NewHelloWorldServiceClient(
			http.DefaultClient,
			serverURL,
		)

		name := "Alice"
		req := connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		})
		req.Header().Set("x-user-id", "user123")

		resp, err := client.SayHello(context.Background(), req)
		if err != nil {
			t.Fatalf("Request with header failed: %v", err)
		}

		if !strings.Contains(resp.Msg.Message, "Alice") {
			t.Fatalf("Response should contain name: %q", resp.Msg.Message)
		}
		if !strings.Contains(resp.Msg.Message, "user123") {
			t.Fatalf("Response should contain user ID: %q", resp.Msg.Message)
		}

		t.Logf("Response: %s", resp.Msg.Message)
	})
}
