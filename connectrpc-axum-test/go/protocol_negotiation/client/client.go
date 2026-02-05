package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"strings"
)

type testCase struct {
	name            string
	method          string
	contentType     string // For POST requests
	query           string // For GET requests
	wantStatus      int
	wantEmptyBody   bool
	wantAcceptPost  bool
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	transport := &http.Transport{
		DialContext: func(_ context.Context, _, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		},
	}
	client := &http.Client{Transport: transport}

	validMessage := url.QueryEscape(`{"name":"Protocol Negotiation Tester"}`)

	testCases := []testCase{
		{
			name:           "POST with Content-Type: text/plain returns 415",
			method:         "POST",
			contentType:    "text/plain",
			wantStatus:     415,
			wantEmptyBody:  true,
			wantAcceptPost: true,
		},
		{
			name:           "POST with Content-Type: application/xml returns 415",
			method:         "POST",
			contentType:    "application/xml",
			wantStatus:     415,
			wantEmptyBody:  true,
			wantAcceptPost: true,
		},
		{
			name:           "GET with unsupported encoding=msgpack returns 415",
			method:         "GET",
			query:          fmt.Sprintf("connect=v1&encoding=msgpack&message=%s", validMessage),
			wantStatus:     415,
			wantEmptyBody:  true,
			wantAcceptPost: true,
		},
	}

	failures := 0
	for _, tc := range testCases {
		err := runTest(client, tc)
		if err != nil {
			fmt.Printf("    FAIL  %s: %v\n", tc.name, err)
			failures++
		} else {
			fmt.Printf("    PASS  %s\n", tc.name)
		}
	}

	if failures > 0 {
		os.Exit(1)
	}
}

func runTest(client *http.Client, tc testCase) error {
	var req *http.Request
	var err error

	if tc.method == "GET" {
		reqURL := "http://localhost/hello.HelloWorldService/GetGreeting?" + tc.query
		req, err = http.NewRequest("GET", reqURL, nil)
	} else {
		reqURL := "http://localhost/hello.HelloWorldService/SayHello"
		req, err = http.NewRequest("POST", reqURL, strings.NewReader(`{"name":"test"}`))
		if tc.contentType != "" {
			req.Header.Set("Content-Type", tc.contentType)
		}
	}
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	// Check status code
	if resp.StatusCode != tc.wantStatus {
		return fmt.Errorf("expected HTTP %d, got %d. Body: %s", tc.wantStatus, resp.StatusCode, string(body))
	}

	// Check empty body for 415 responses
	if tc.wantEmptyBody && len(body) != 0 {
		return fmt.Errorf("expected empty body for HTTP %d, got %q (len=%d)", tc.wantStatus, string(body), len(body))
	}

	// Check Accept-Post header
	if tc.wantAcceptPost {
		acceptPost := resp.Header.Get("Accept-Post")
		if acceptPost == "" {
			return fmt.Errorf("expected Accept-Post header, but not present")
		}
		if !strings.Contains(acceptPost, "application/json") && !strings.Contains(acceptPost, "application/connect+json") {
			return fmt.Errorf("Accept-Post %q doesn't contain expected content types", acceptPost)
		}
	}

	return nil
}
