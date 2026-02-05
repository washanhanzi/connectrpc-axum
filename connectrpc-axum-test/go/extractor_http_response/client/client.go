package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
)

type testCase struct {
	name           string
	includeUserID  bool
	expectedStatus int
	// Only checked when expectedStatus is 200
	expectedMessageContains []string
}

var testCases = []testCase{
	{
		name:           "without x-user-id returns 401",
		includeUserID:  false,
		expectedStatus: 401,
	},
	{
		name:                    "with x-user-id returns success",
		includeUserID:           true,
		expectedStatus:          200,
		expectedMessageContains: []string{"Alice", "user123"},
	},
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
	url := "http://localhost/hello.HelloWorldService/SayHello"

	req, err := http.NewRequest("POST", url, strings.NewReader(`{"name":"Alice"}`))
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")

	if tc.includeUserID {
		req.Header.Set("x-user-id", "user123")
	}

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != tc.expectedStatus {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("expected status %d, got %d, body: %s", tc.expectedStatus, resp.StatusCode, string(body))
	}

	// For success case, validate the response message
	if tc.expectedStatus == 200 {
		body, err := io.ReadAll(resp.Body)
		if err != nil {
			return fmt.Errorf("reading body: %w", err)
		}

		var jsonResp map[string]any
		if err := json.Unmarshal(body, &jsonResp); err != nil {
			return fmt.Errorf("invalid JSON: %s", string(body))
		}

		msg, ok := jsonResp["message"].(string)
		if !ok || msg == "" {
			return fmt.Errorf("expected message field, got: %s", string(body))
		}

		for _, expected := range tc.expectedMessageContains {
			if !strings.Contains(msg, expected) {
				return fmt.Errorf("expected message to contain %q, got: %q", expected, msg)
			}
		}
	}

	return nil
}
