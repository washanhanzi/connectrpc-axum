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
	name            string
	requestBody     string
	expectedCode    string
	expectedMessage string
}

var testCases = []testCase{
	{"error with details", `{"name":"Alice"}`, "invalid_argument", "name is required"},
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

	req, err := http.NewRequest("POST", url, strings.NewReader(tc.requestBody))
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")

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

	// Validate the error code
	code, ok := jsonResp["code"].(string)
	if !ok || code == "" {
		return fmt.Errorf("expected code field, got: %s", string(body))
	}
	if code != tc.expectedCode {
		return fmt.Errorf("expected code %q, got %q", tc.expectedCode, code)
	}

	// Validate the error message
	msg, ok := jsonResp["message"].(string)
	if !ok || msg == "" {
		return fmt.Errorf("expected message field, got: %s", string(body))
	}
	if msg != tc.expectedMessage {
		return fmt.Errorf("expected message %q, got %q", tc.expectedMessage, msg)
	}

	// Validate the error details array is present and non-empty
	details, ok := jsonResp["details"].([]any)
	if !ok {
		return fmt.Errorf("expected details array, got: %s", string(body))
	}
	if len(details) == 0 {
		return fmt.Errorf("expected non-empty details array, got empty")
	}

	return nil
}
