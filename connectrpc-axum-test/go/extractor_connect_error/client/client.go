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
	name          string
	includeUserID bool
	expectError   bool
}

var testCases = []testCase{
	{"without x-user-id header returns unauthenticated", false, true},
	{"with x-user-id header succeeds", true, false},
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

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	var jsonResp map[string]any
	if err := json.Unmarshal(body, &jsonResp); err != nil {
		return fmt.Errorf("invalid JSON: %s", string(body))
	}

	if tc.expectError {
		// Expect a Connect error with code "unauthenticated"
		code, ok := jsonResp["code"].(string)
		if !ok || code != "unauthenticated" {
			return fmt.Errorf("expected code unauthenticated, got: %s", string(body))
		}

		msg, _ := jsonResp["message"].(string)
		if !strings.Contains(msg, "x-user-id") {
			return fmt.Errorf("expected error message to mention x-user-id, got: %s", string(body))
		}
	} else {
		// Expect success
		msg, ok := jsonResp["message"].(string)
		if !ok || msg == "" {
			return fmt.Errorf("expected message field, got: %s", string(body))
		}

		if !strings.Contains(msg, "Alice") {
			return fmt.Errorf("expected message to contain 'Alice', got %q", msg)
		}
		if !strings.Contains(msg, "user123") {
			return fmt.Errorf("expected message to contain 'user123', got %q", msg)
		}
	}

	return nil
}
