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
	"time"
)

type testCase struct {
	name          string
	timeoutMs     int // 0 means no header
	expectSuccess bool
}

var testCases = []testCase{
	{"short timeout fails", 100, false},
	{"long timeout succeeds", 1000, true},
	{"no timeout succeeds", 0, true},
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	transport := &http.Transport{
		DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		},
	}
	client := &http.Client{
		Transport: transport,
		Timeout:   5 * time.Second,
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
	url := "http://localhost/hello.HelloWorldService/SayHello"

	req, err := http.NewRequest("POST", url, strings.NewReader(`{"name":"Timeout Tester"}`))
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")
	if tc.timeoutMs > 0 {
		req.Header.Set("Connect-Timeout-Ms", fmt.Sprintf("%d", tc.timeoutMs))
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

	if tc.expectSuccess {
		if msg, ok := jsonResp["message"].(string); ok && msg != "" {
			return nil
		}
		return fmt.Errorf("expected success with message, got: %s", string(body))
	}

	code, ok := jsonResp["code"].(string)
	if !ok || code != "deadline_exceeded" {
		return fmt.Errorf("expected deadline_exceeded, got: %s", string(body))
	}
	return nil
}
