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
)

type testCase struct {
	name            string
	uri             string
	expectSuccess   bool
	expectedMessage string
}

var testCases = []testCase{
	{
		"GET with JSON encoding and name",
		// For encoding=json, the message param is raw JSON (URL-encoded in query string)
		"/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&message=%7B%22name%22%3A%22Alice%22%7D",
		true,
		"Hello, Alice!",
	},
	{
		"GET with JSON encoding and no name",
		// message={} as raw JSON in query string
		"/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&message=%7B%7D",
		true,
		"Hello, World!",
	},
	{
		"GET with base64-encoded JSON message",
		// encoding=json with base64=1: message is base64url-encoded JSON
		// {"name":"Alice"} -> base64url: eyJuYW1lIjoiQWxpY2UifQ
		"/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&base64=1&message=eyJuYW1lIjoiQWxpY2UifQ",
		true,
		"Hello, Alice!",
	},
	{
		"GET missing message parameter",
		"/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1",
		false,
		"",
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
	url := "http://localhost" + tc.uri

	req, err := http.NewRequest("GET", url, nil)
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

	var jsonResp map[string]any
	if err := json.Unmarshal(body, &jsonResp); err != nil {
		if !tc.expectSuccess {
			// For error cases, non-JSON responses are acceptable
			if resp.StatusCode != 200 {
				return nil
			}
		}
		return fmt.Errorf("invalid JSON (status %d): %s", resp.StatusCode, string(body))
	}

	if tc.expectSuccess {
		if resp.StatusCode != 200 {
			return fmt.Errorf("expected HTTP 200, got %d: %s", resp.StatusCode, string(body))
		}
		msg, ok := jsonResp["message"].(string)
		if !ok || msg == "" {
			return fmt.Errorf("expected message field, got: %s", string(body))
		}
		if msg != tc.expectedMessage {
			return fmt.Errorf("expected message %q, got %q", tc.expectedMessage, msg)
		}
		return nil
	}

	// Expect failure
	if resp.StatusCode == 200 {
		return fmt.Errorf("expected non-200 status, got 200: %s", string(body))
	}
	return nil
}
