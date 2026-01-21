package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"google.golang.org/protobuf/proto"
)

// TestIdempotencyAutoGet verifies that methods marked with NO_SIDE_EFFECTS
// automatically support HTTP GET requests.
//
// The Rust server uses the generated service builder which auto-enables GET
// for methods with idempotency_level = NO_SIDE_EFFECTS in the proto.
func TestIdempotencyAutoGet(t *testing.T) {
	s := startServer(t, "idempotency-get", "")
	defer s.stop()

	// Test 1: Connect client with HTTPGet enabled (default for NO_SIDE_EFFECTS)
	t.Run("connect_client_uses_get", func(t *testing.T) {
		client := genconnect.NewHelloWorldServiceClient(
			http.DefaultClient,
			serverURL,
			connect.WithHTTPGet(), // Enable GET for idempotent methods
		)

		req := connect.NewRequest(&gen.HelloRequest{
			Name: proto.String("Connect Client"),
		})

		resp, err := client.GetGreeting(context.Background(), req)
		if err != nil {
			t.Fatalf("GetGreeting failed: %v", err)
		}

		t.Logf("Response: %s", resp.Msg.Message)
		if resp.Msg.Message == "" {
			t.Error("Empty response message")
		}
	})

	// Test 2: Raw HTTP GET request (JSON encoding)
	t.Run("raw_http_get_json", func(t *testing.T) {
		baseURL := serverURL + "/hello.HelloWorldService/GetGreeting"
		message := url.QueryEscape(`{"name":"Raw GET"}`)
		reqURL := fmt.Sprintf("%s?connect=v1&encoding=json&message=%s", baseURL, message)

		resp, err := http.Get(reqURL)
		if err != nil {
			t.Fatalf("HTTP GET failed: %v", err)
		}
		defer resp.Body.Close()

		body, err := io.ReadAll(resp.Body)
		if err != nil {
			t.Fatalf("Failed to read body: %v", err)
		}

		if resp.StatusCode != 200 {
			t.Fatalf("Status = %d, want 200. Body: %s", resp.StatusCode, string(body))
		}

		var result struct {
			Message string `json:"message"`
		}
		if err := json.Unmarshal(body, &result); err != nil {
			t.Fatalf("Failed to parse JSON: %v. Body: %s", err, string(body))
		}

		t.Logf("Response: %s", result.Message)
		if result.Message == "" {
			t.Error("Empty response message")
		}
	})

	// Test 3: Raw HTTP GET request (proto encoding with base64)
	t.Run("raw_http_get_proto", func(t *testing.T) {
		baseURL := serverURL + "/hello.HelloWorldService/GetGreeting"

		// Create protobuf message
		msg := &gen.HelloRequest{Name: proto.String("Proto GET")}
		msgBytes, err := proto.Marshal(msg)
		if err != nil {
			t.Fatalf("Failed to marshal proto: %v", err)
		}

		// URL-safe base64 encode
		encoded := base64.URLEncoding.EncodeToString(msgBytes)
		reqURL := fmt.Sprintf("%s?connect=v1&encoding=proto&base64=1&message=%s", baseURL, url.QueryEscape(encoded))

		resp, err := http.Get(reqURL)
		if err != nil {
			t.Fatalf("HTTP GET failed: %v", err)
		}
		defer resp.Body.Close()

		body, err := io.ReadAll(resp.Body)
		if err != nil {
			t.Fatalf("Failed to read body: %v", err)
		}

		if resp.StatusCode != 200 {
			t.Fatalf("Status = %d, want 200. Body: %s", resp.StatusCode, string(body))
		}

		// Parse protobuf response
		var result gen.HelloResponse
		if err := proto.Unmarshal(body, &result); err != nil {
			t.Fatalf("Failed to unmarshal proto: %v", err)
		}

		t.Logf("Response: %s", result.Message)
		if result.Message == "" {
			t.Error("Empty response message")
		}
	})

	// Test 4: POST still works
	t.Run("post_still_works", func(t *testing.T) {
		client := genconnect.NewHelloWorldServiceClient(
			http.DefaultClient,
			serverURL,
			// No HTTPGet option - will use POST
		)

		req := connect.NewRequest(&gen.HelloRequest{
			Name: proto.String("POST Client"),
		})

		resp, err := client.GetGreeting(context.Background(), req)
		if err != nil {
			t.Fatalf("GetGreeting via POST failed: %v", err)
		}

		t.Logf("Response: %s", resp.Msg.Message)
		if resp.Msg.Message == "" {
			t.Error("Empty response message")
		}
	})
}

// TestIdempotencyLevelConstant verifies the generated constant is accessible.
// This is a compile-time check that the constant exists.
func TestIdempotencyLevelConstant(t *testing.T) {
	// The procedure constant should be accessible
	procedure := genconnect.HelloWorldServiceGetGreetingProcedure
	if procedure != "/hello.HelloWorldService/GetGreeting" {
		t.Errorf("Procedure = %q, want /hello.HelloWorldService/GetGreeting", procedure)
	}
	t.Logf("GetGreeting procedure: %s", procedure)
}
