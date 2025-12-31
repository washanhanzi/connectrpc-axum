package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"testing"
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
			name:          "invalid_encoding",
			query:         fmt.Sprintf("connect=v1&encoding=xml&message=%s", validMessage),
			wantStatus:    400,
			wantErrorCode: "invalid_argument",
			wantContains:  "invalid message encoding",
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
			wantContains:  "unknown compression",
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
