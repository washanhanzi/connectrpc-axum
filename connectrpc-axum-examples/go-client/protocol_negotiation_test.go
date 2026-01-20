package main

import (
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"testing"
)

// TestProtocolNegotiation415 verifies HTTP 415 Unsupported Media Type responses
// for unsupported content-types and invalid GET encodings.
//
// Per connect-go behavior, unsupported content-types and invalid GET encodings
// return HTTP 415 with an Accept-Post header listing supported content types.
func TestProtocolNegotiation415(t *testing.T) {
	s := startServer(t, "get-request", "")
	defer s.stop()

	baseURL := serverURL + "/hello.HelloWorldService/SayHello"

	// Valid JSON message payload (URL-encoded)
	validMessage := url.QueryEscape(`{"name":"Protocol Negotiation Tester"}`)

	tests := []struct {
		name             string
		method           string
		contentType      string // For POST requests
		query            string // For GET requests
		wantStatus       int
		wantAcceptPost   bool   // Should have Accept-Post header
		wantAcceptPostContains []string // Values that Accept-Post should contain
	}{
		// POST with unsupported Content-Type should return 415
		{
			name:           "post_unsupported_content_type_xml",
			method:         "POST",
			contentType:    "application/xml",
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
				"application/proto",
				"application/connect+json",
				"application/connect+proto",
			},
		},
		{
			name:           "post_unsupported_content_type_text_plain",
			method:         "POST",
			contentType:    "text/plain",
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
				"application/proto",
			},
		},
		{
			name:           "post_unsupported_content_type_form",
			method:         "POST",
			contentType:    "application/x-www-form-urlencoded",
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
			},
		},
		{
			name:           "post_empty_content_type",
			method:         "POST",
			contentType:    "",
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
			},
		},
		// POST with supported Content-Type should work (or fail later in processing)
		{
			name:           "post_supported_content_type_json",
			method:         "POST",
			contentType:    "application/json",
			wantStatus:     200, // Valid request with empty body might be 400, but not 415
			wantAcceptPost: false,
		},
		// GET with invalid encoding should return 415
		{
			name:           "get_invalid_encoding_xml",
			method:         "GET",
			query:          fmt.Sprintf("connect=v1&encoding=xml&message=%s", validMessage),
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
				"application/proto",
			},
		},
		{
			name:           "get_invalid_encoding_yaml",
			method:         "GET",
			query:          fmt.Sprintf("connect=v1&encoding=yaml&message=%s", validMessage),
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
			},
		},
		{
			name:           "get_invalid_encoding_empty",
			method:         "GET",
			query:          fmt.Sprintf("connect=v1&encoding=&message=%s", validMessage),
			wantStatus:     415,
			wantAcceptPost: true,
			wantAcceptPostContains: []string{
				"application/json",
			},
		},
		// GET with valid encoding should work
		{
			name:           "get_valid_encoding_json",
			method:         "GET",
			query:          fmt.Sprintf("connect=v1&encoding=json&message=%s", validMessage),
			wantStatus:     200,
			wantAcceptPost: false,
		},
		{
			name:           "get_valid_encoding_proto",
			method:         "GET",
			query:          fmt.Sprintf("connect=v1&encoding=proto&message=%s", validMessage),
			wantStatus:     400, // Invalid protobuf message, but not 415
			wantAcceptPost: false,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			var req *http.Request
			var err error

			if tc.method == "GET" {
				reqURL := baseURL + "?" + tc.query
				req, err = http.NewRequest("GET", reqURL, nil)
			} else {
				req, err = http.NewRequest("POST", baseURL, strings.NewReader(`{"name":"test"}`))
				if tc.contentType != "" {
					req.Header.Set("Content-Type", tc.contentType)
				}
			}
			if err != nil {
				t.Fatalf("Failed to create request: %v", err)
			}

			resp, err := http.DefaultClient.Do(req)
			if err != nil {
				t.Fatalf("HTTP request failed: %v", err)
			}
			defer resp.Body.Close()

			body, err := io.ReadAll(resp.Body)
			if err != nil {
				t.Fatalf("Failed to read response body: %v", err)
			}

			// Check status code
			if resp.StatusCode != tc.wantStatus {
				t.Errorf("Status = %d, want %d. Body: %s", resp.StatusCode, tc.wantStatus, string(body))
			}

			// Check Accept-Post header
			acceptPost := resp.Header.Get("Accept-Post")
			if tc.wantAcceptPost {
				if acceptPost == "" {
					t.Error("Expected Accept-Post header, but not present")
				} else {
					t.Logf("Accept-Post: %s", acceptPost)
					for _, want := range tc.wantAcceptPostContains {
						if !strings.Contains(acceptPost, want) {
							t.Errorf("Accept-Post %q doesn't contain %q", acceptPost, want)
						}
					}
				}
			} else {
				if acceptPost != "" && tc.wantStatus != 415 {
					// Accept-Post might be present even for non-415 in some cases
					t.Logf("Accept-Post present (unexpected but not an error): %s", acceptPost)
				}
			}

			if tc.wantStatus == 415 {
				t.Logf("415 response body (should be empty or minimal): %q", string(body))
				// 415 responses should have empty or minimal body per connect-go behavior
			} else if tc.wantStatus == 200 {
				t.Logf("Success response: %s", string(body))
			} else {
				t.Logf("Error response: %s", string(body))
			}
		})
	}
}

// TestProtocolNegotiation415EmptyBody verifies that HTTP 415 responses have empty body.
//
// Per connect-go behavior, 415 responses don't contain a Connect error body,
// just the HTTP status and Accept-Post header.
func TestProtocolNegotiation415EmptyBody(t *testing.T) {
	s := startServer(t, "get-request", "")
	defer s.stop()

	baseURL := serverURL + "/hello.HelloWorldService/SayHello"

	// POST with unsupported Content-Type
	req, err := http.NewRequest("POST", baseURL, strings.NewReader("some body"))
	if err != nil {
		t.Fatalf("Failed to create request: %v", err)
	}
	req.Header.Set("Content-Type", "application/xml")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("HTTP request failed: %v", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("Failed to read response body: %v", err)
	}

	if resp.StatusCode != 415 {
		t.Fatalf("Status = %d, want 415. Body: %s", resp.StatusCode, string(body))
	}

	// Body should be empty for 415 responses
	if len(body) != 0 {
		t.Errorf("Expected empty body for 415, got %q (len=%d)", string(body), len(body))
	}

	// Accept-Post header should be present
	acceptPost := resp.Header.Get("Accept-Post")
	if acceptPost == "" {
		t.Error("Expected Accept-Post header, but not present")
	}
	t.Logf("Accept-Post: %s", acceptPost)
}
