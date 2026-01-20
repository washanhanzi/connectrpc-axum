package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"testing"

	"github.com/connectrpc-axum/examples/go-client/gen"
	"google.golang.org/protobuf/proto"
)

// TestGetRequestValidation verifies GET unary request validation.
//
// Tests that the server correctly validates GET request query parameters:
// - encoding: required, must be "json" or "proto"
// - message: required
// - connect: must be "v1" if present
// - compression: if present, must be "gzip" or "identity"
func TestGetRequestValidation(t *testing.T) {
	s := startServer(t, "get-request", "")
	defer s.stop()

	baseURL := serverURL + "/hello.HelloWorldService/SayHello"

	// Valid JSON message payload (URL-encoded)
	validMessage := url.QueryEscape(`{"name":"GET Tester"}`)

	tests := []struct {
		name           string
		query          string
		wantStatus     int
		wantErrorCode  string // Connect error code in response body
		wantContains   string // Error message should contain this
	}{
		{
			name:       "valid_get_request",
			query:      fmt.Sprintf("connect=v1&encoding=json&message=%s", validMessage),
			wantStatus: 200,
		},
		{
			name:       "valid_get_without_connect_param",
			query:      fmt.Sprintf("encoding=json&message=%s", validMessage),
			wantStatus: 200, // connect param is optional by default
		},
		{
			name:          "missing_encoding",
			query:         fmt.Sprintf("connect=v1&message=%s", validMessage),
			wantStatus:    400,
			wantErrorCode: "invalid_argument",
			wantContains:  "missing encoding parameter",
		},
		{
			name:       "invalid_encoding",
			query:      fmt.Sprintf("connect=v1&encoding=xml&message=%s", validMessage),
			wantStatus: 415, // HTTP 415 Unsupported Media Type with Accept-Post header
			// No error code in body - 415 responses have empty body
		},
		{
			name:          "missing_message",
			query:         "connect=v1&encoding=json",
			wantStatus:    400,
			wantErrorCode: "invalid_argument",
			wantContains:  "missing message parameter",
		},
		{
			name:          "invalid_connect_version",
			query:         fmt.Sprintf("connect=v2&encoding=json&message=%s", validMessage),
			wantStatus:    400,
			wantErrorCode: "invalid_argument",
			wantContains:  "connect must be",
		},
		{
			name:          "unsupported_compression",
			query:         fmt.Sprintf("connect=v1&encoding=json&compression=br&message=%s", validMessage),
			wantStatus:    501,
			wantErrorCode: "unimplemented",
			wantContains:  "unsupported compression", // Message from resolve_codec
		},
		{
			name:          "gzip_compression_with_uncompressed_message",
			query:         fmt.Sprintf("connect=v1&encoding=json&compression=gzip&message=%s", validMessage),
			wantStatus:    400, // Message must be actually gzipped when compression=gzip
			wantErrorCode: "invalid_argument",
			wantContains:  "decompression failed",
		},
		{
			name:       "valid_with_identity_compression",
			query:      fmt.Sprintf("connect=v1&encoding=json&compression=identity&message=%s", validMessage),
			wantStatus: 200,
		},
		{
			name:       "empty_message_is_valid",
			query:      "connect=v1&encoding=json&message=",
			wantStatus: 400, // Empty JSON is invalid, but message param present is valid
			// The error here will be about JSON parsing, not missing message
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			reqURL := baseURL + "?" + tc.query
			resp, err := http.Get(reqURL)
			if err != nil {
				t.Fatalf("HTTP request failed: %v", err)
			}
			defer resp.Body.Close()

			body, err := io.ReadAll(resp.Body)
			if err != nil {
				t.Fatalf("Failed to read response body: %v", err)
			}

			if resp.StatusCode != tc.wantStatus {
				t.Errorf("Status = %d, want %d. Body: %s", resp.StatusCode, tc.wantStatus, string(body))
				return
			}

			// For error cases, verify the error response
			if tc.wantStatus != 200 && tc.wantErrorCode != "" {
				var errResp struct {
					Code    string `json:"code"`
					Message string `json:"message"`
				}
				if err := json.Unmarshal(body, &errResp); err != nil {
					t.Errorf("Failed to parse error response: %v. Body: %s", err, string(body))
					return
				}

				if errResp.Code != tc.wantErrorCode {
					t.Errorf("Error code = %q, want %q", errResp.Code, tc.wantErrorCode)
				}

				if tc.wantContains != "" && errResp.Message != "" {
					if !contains(errResp.Message, tc.wantContains) {
						t.Errorf("Error message %q doesn't contain %q", errResp.Message, tc.wantContains)
					}
				}
			}

			// For success cases, verify we got a valid response
			if tc.wantStatus == 200 {
				var successResp struct {
					Message string `json:"message"`
				}
				if err := json.Unmarshal(body, &successResp); err != nil {
					t.Errorf("Failed to parse success response: %v. Body: %s", err, string(body))
					return
				}
				if successResp.Message == "" {
					t.Error("Empty response message for successful request")
				}
				t.Logf("Success: %s", successResp.Message)
			} else {
				t.Logf("Expected error: %s", string(body))
			}
		})
	}
}

// contains checks if s contains substr (case-insensitive would be better but this is simpler)
func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(substr) == 0 ||
		(len(s) > 0 && containsAt(s, substr)))
}

func containsAt(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// TestGetRequestBase64Encoding verifies GET unary requests with base64-encoded protobuf messages.
//
// Tests that the server correctly handles both padded and unpadded URL-safe base64 encoding,
// matching the connect-go reference implementation behavior.
func TestGetRequestBase64Encoding(t *testing.T) {
	s := startServer(t, "get-request", "")
	defer s.stop()

	baseURL := serverURL + "/hello.HelloWorldService/SayHello"

	// Create a protobuf message
	msg := &gen.HelloRequest{Name: proto.String("Base64 Tester")}
	msgBytes, err := proto.Marshal(msg)
	if err != nil {
		t.Fatalf("Failed to marshal protobuf: %v", err)
	}

	// Encode with padding (standard URL-safe base64)
	paddedBase64 := base64.URLEncoding.EncodeToString(msgBytes)
	// Encode without padding (raw URL-safe base64)
	unpaddedBase64 := base64.RawURLEncoding.EncodeToString(msgBytes)

	t.Logf("Original bytes length: %d", len(msgBytes))
	t.Logf("Padded base64: %q (len=%d)", paddedBase64, len(paddedBase64))
	t.Logf("Unpadded base64: %q (len=%d)", unpaddedBase64, len(unpaddedBase64))

	tests := []struct {
		name       string
		base64Msg  string
		wantStatus int
	}{
		{
			name:       "padded_base64",
			base64Msg:  paddedBase64,
			wantStatus: 200,
		},
		{
			name:       "unpadded_base64",
			base64Msg:  unpaddedBase64,
			wantStatus: 200,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			query := fmt.Sprintf("connect=v1&encoding=proto&base64=1&message=%s", url.QueryEscape(tc.base64Msg))
			reqURL := baseURL + "?" + query
			resp, err := http.Get(reqURL)
			if err != nil {
				t.Fatalf("HTTP request failed: %v", err)
			}
			defer resp.Body.Close()

			body, err := io.ReadAll(resp.Body)
			if err != nil {
				t.Fatalf("Failed to read response body: %v", err)
			}

			if resp.StatusCode != tc.wantStatus {
				t.Errorf("Status = %d, want %d. Body: %s", resp.StatusCode, tc.wantStatus, string(body))
				return
			}

			// Parse as protobuf response
			var helloResp gen.HelloResponse
			if err := proto.Unmarshal(body, &helloResp); err != nil {
				t.Errorf("Failed to unmarshal protobuf response: %v. Body (hex): %x", err, body)
				return
			}

			t.Logf("Success: %s", helloResp.Message)
			if helloResp.Message == "" {
				t.Error("Empty response message")
			}
		})
	}
}

// TestGetRequestBase64EdgeCases tests edge cases for base64 encoding where
// the padding makes a difference (length % 4 != 0 for unpadded).
func TestGetRequestBase64EdgeCases(t *testing.T) {
	s := startServer(t, "get-request", "")
	defer s.stop()

	baseURL := serverURL + "/hello.HelloWorldService/SayHello"

	// Test different name lengths to hit different padding scenarios
	// Base64 output length depends on input: ceil(n * 4/3) rounded to multiple of 4 with padding
	testNames := []string{
		"A",      // Very short - different padding
		"AB",     // 2 chars
		"ABC",    // 3 chars - no padding needed
		"ABCD",   // 4 chars
		"Hello",  // 5 chars
		"World!", // 6 chars
	}

	for _, name := range testNames {
		msg := &gen.HelloRequest{Name: proto.String(name)}
		msgBytes, err := proto.Marshal(msg)
		if err != nil {
			t.Fatalf("Failed to marshal protobuf: %v", err)
		}

		paddedBase64 := base64.URLEncoding.EncodeToString(msgBytes)
		unpaddedBase64 := base64.RawURLEncoding.EncodeToString(msgBytes)

		// Test both padded and unpadded for each name length
		for _, tc := range []struct {
			variant   string
			base64Msg string
		}{
			{"padded", paddedBase64},
			{"unpadded", unpaddedBase64},
		} {
			testName := fmt.Sprintf("name=%q_%s_len%%4=%d", name, tc.variant, len(tc.base64Msg)%4)
			t.Run(testName, func(t *testing.T) {
				query := fmt.Sprintf("connect=v1&encoding=proto&base64=1&message=%s", url.QueryEscape(tc.base64Msg))
				reqURL := baseURL + "?" + query
				resp, err := http.Get(reqURL)
				if err != nil {
					t.Fatalf("HTTP request failed: %v", err)
				}
				defer resp.Body.Close()

				body, err := io.ReadAll(resp.Body)
				if err != nil {
					t.Fatalf("Failed to read response body: %v", err)
				}

				if resp.StatusCode != 200 {
					t.Errorf("Status = %d, want 200. Body: %s", resp.StatusCode, string(body))
					return
				}

				var helloResp gen.HelloResponse
				if err := proto.Unmarshal(body, &helloResp); err != nil {
					t.Errorf("Failed to unmarshal protobuf response: %v", err)
					return
				}

				t.Logf("name=%q, %s (len=%d, %%4=%d): %s",
					name, tc.variant, len(tc.base64Msg), len(tc.base64Msg)%4, helloResp.Message)
			})
		}
	}
}
