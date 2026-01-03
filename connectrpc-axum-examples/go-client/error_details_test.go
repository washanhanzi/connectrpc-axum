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
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/protobuf/proto"
)

// TestErrorDetails verifies that ConnectError details are serialized correctly
// according to the Connect protocol specification.
//
// The Connect protocol requires error details to be serialized as:
//
//	{
//	  "code": "resource_exhausted",
//	  "message": "rate limited",
//	  "details": [
//	    {"type": "google.rpc.RetryInfo", "value": "base64-encoded-protobuf"}
//	  ]
//	}
//
// NOT as just base64 strings:
//
//	{
//	  "code": "resource_exhausted",
//	  "details": ["base64-string"]  // WRONG!
//	}
func TestErrorDetails(t *testing.T) {
	s := startServer(t, "error-details", "")
	defer s.stop()

	t.Run("raw_response_format", func(t *testing.T) {
		// Use raw HTTP to inspect the actual wire format
		req, err := http.NewRequest("POST", serverURL+"/hello.HelloWorldService/SayHello", strings.NewReader(`{"name":"error"}`))
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/json")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should be HTTP 429 (ResourceExhausted)
		if resp.StatusCode != http.StatusTooManyRequests {
			t.Fatalf("Expected 429 Too Many Requests, got: %d", resp.StatusCode)
		}

		body, _ := io.ReadAll(resp.Body)
		t.Logf("Raw response body: %s", string(body))

		// Parse the response as JSON
		var errResp struct {
			Code    string            `json:"code"`
			Message string            `json:"message"`
			Details []json.RawMessage `json:"details"`
		}
		if err := json.Unmarshal(body, &errResp); err != nil {
			t.Fatalf("Failed to parse error response: %v", err)
		}

		if errResp.Code != "resource_exhausted" {
			t.Fatalf("Expected code 'resource_exhausted', got %q", errResp.Code)
		}

		if len(errResp.Details) == 0 {
			t.Fatal("Expected error details to be present")
		}

		// Check the format of the first detail
		var detail struct {
			Type  string `json:"type"`
			Value string `json:"value"`
		}
		if err := json.Unmarshal(errResp.Details[0], &detail); err != nil {
			// If we can't unmarshal as {type, value} object, it's the bug!
			// The current implementation serializes as just a base64 string
			var rawString string
			if json.Unmarshal(errResp.Details[0], &rawString) == nil {
				t.Fatalf("BUG: details[0] is a raw string %q instead of {type, value} object. "+
					"Connect protocol requires details to be objects with 'type' and 'value' fields.",
					rawString)
			}
			t.Fatalf("Failed to parse detail: %v", err)
		}

		// Verify the type field
		if detail.Type != "google.rpc.RetryInfo" {
			t.Fatalf("Expected detail type 'google.rpc.RetryInfo', got %q", detail.Type)
		}

		t.Logf("Detail type: %s, value: %s", detail.Type, detail.Value)
	})

	t.Run("connect_client_can_decode_details", func(t *testing.T) {
		client := genconnect.NewHelloWorldServiceClient(
			http.DefaultClient,
			serverURL,
		)

		name := "error"
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

		if connectErr.Code() != connect.CodeResourceExhausted {
			t.Fatalf("Expected CodeResourceExhausted, got: %v", connectErr.Code())
		}

		// Get error details
		details := connectErr.Details()
		if len(details) == 0 {
			t.Fatal("Expected error to have details")
		}

		t.Logf("Number of details: %d", len(details))
		t.Logf("Detail[0] type: %s", details[0].Type())

		// Verify the detail type
		if details[0].Type() != "google.rpc.RetryInfo" {
			t.Fatalf("Expected detail type 'google.rpc.RetryInfo', got %q", details[0].Type())
		}

		// Try to unmarshal as RetryInfo
		msg, err := details[0].Value()
		if err != nil {
			t.Fatalf("Failed to get detail value: %v", err)
		}

		retryInfo, ok := msg.(*errdetails.RetryInfo)
		if !ok {
			t.Fatalf("Expected *errdetails.RetryInfo, got %T", msg)
		}

		// Verify the retry delay is 5 seconds
		if retryInfo.RetryDelay == nil {
			t.Fatal("Expected RetryDelay to be set")
		}

		if retryInfo.RetryDelay.Seconds != 5 {
			t.Fatalf("Expected RetryDelay.Seconds to be 5, got %d", retryInfo.RetryDelay.Seconds)
		}

		t.Logf("RetryInfo decoded successfully: retry_delay=%v", retryInfo.RetryDelay)
	})

	t.Run("success_response_has_no_details", func(t *testing.T) {
		client := genconnect.NewHelloWorldServiceClient(
			http.DefaultClient,
			serverURL,
		)

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
}

// Ensure errdetails types are registered
var _ proto.Message = (*errdetails.RetryInfo)(nil)
