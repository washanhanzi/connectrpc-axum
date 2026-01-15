package main

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestAxumRouterHealth tests that plain axum routes bypass ConnectLayer.
// The /health endpoint should return JSON without requiring Connect headers.
func TestAxumRouterHealth(t *testing.T) {
	s := startServer(t, "axum-router", "")
	defer s.stop()

	// Test health endpoint - plain GET request without Connect headers
	resp, err := http.Get(serverURL + "/health")
	if err != nil {
		t.Fatalf("Health check failed: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("Expected status 200, got %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("Failed to read response: %v", err)
	}

	// Parse JSON response
	var health struct {
		Status string `json:"status"`
	}
	if err := json.Unmarshal(body, &health); err != nil {
		t.Fatalf("Failed to parse JSON: %v (body: %s)", err, string(body))
	}

	if health.Status != "ok" {
		t.Fatalf("Expected status 'ok', got %q", health.Status)
	}

	t.Logf("Health check response: %s", string(body))
}

// TestAxumRouterMetrics tests the plain text metrics endpoint.
func TestAxumRouterMetrics(t *testing.T) {
	s := startServer(t, "axum-router", "")
	defer s.stop()

	// Test metrics endpoint - plain GET request
	resp, err := http.Get(serverURL + "/metrics")
	if err != nil {
		t.Fatalf("Metrics request failed: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("Expected status 200, got %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("Failed to read response: %v", err)
	}

	bodyStr := string(body)
	if !strings.Contains(bodyStr, "requests_total") {
		t.Fatalf("Expected metrics output, got: %s", bodyStr)
	}

	t.Logf("Metrics response: %s", bodyStr)
}

// TestAxumRouterConnectRPC tests that Connect RPC still works alongside axum routes.
func TestAxumRouterConnectRPC(t *testing.T) {
	s := startServer(t, "axum-router", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Axum Router Tester"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Connect RPC failed: %v", err)
	}

	if resp.Msg.Message == "" {
		t.Fatal("Empty response message")
	}
	if !strings.Contains(resp.Msg.Message, name) {
		t.Fatalf("Response doesn't contain name: got %q", resp.Msg.Message)
	}

	t.Logf("Connect RPC response: %s", resp.Msg.Message)
}
