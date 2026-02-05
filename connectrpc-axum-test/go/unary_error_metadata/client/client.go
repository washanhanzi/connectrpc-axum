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

	err := runTest(client)
	if err != nil {
		fmt.Printf("    FAIL  error response includes metadata: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  error response includes metadata\n")
}

func runTest(client *http.Client) error {
	url := "http://localhost/hello.HelloWorldService/SayHello"

	req, err := http.NewRequest("POST", url, strings.NewReader(`{"name":"Alice"}`))
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
	if !ok || code != "invalid_argument" {
		return fmt.Errorf("expected code invalid_argument, got: %s", string(body))
	}

	// Validate the error message
	msg, ok := jsonResp["message"].(string)
	if !ok || msg != "name is required" {
		return fmt.Errorf("expected message 'name is required', got: %s", string(body))
	}

	// Validate custom metadata is present in response headers
	customMeta := resp.Header.Get("X-Custom-Meta")
	if customMeta != "custom-value" {
		return fmt.Errorf("expected x-custom-meta 'custom-value', got %q (all headers: %v)", customMeta, resp.Header)
	}

	requestID := resp.Header.Get("X-Request-Id")
	if requestID != "test-123" {
		return fmt.Errorf("expected x-request-id 'test-123', got %q (all headers: %v)", requestID, resp.Header)
	}

	return nil
}
