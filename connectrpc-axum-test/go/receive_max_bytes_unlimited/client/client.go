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
	requestBody   string
	expectSuccess bool
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

	// Generate ~1MB and ~2MB names
	largeName1MB := strings.Repeat("A", 1_000_000)
	largeName2MB := strings.Repeat("B", 2_000_000)

	testCases := []testCase{
		{"large unary request succeeds with unlimited receive size", fmt.Sprintf(`{"name":"%s"}`, largeName1MB), true},
		{"very large unary request succeeds with unlimited receive size", fmt.Sprintf(`{"name":"%s"}`, largeName2MB), true},
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

	if tc.expectSuccess {
		if resp.StatusCode != 200 {
			return fmt.Errorf("expected HTTP 200, got %d: %s", resp.StatusCode, string(body))
		}
		msg, ok := jsonResp["message"].(string)
		if !ok || msg == "" {
			return fmt.Errorf("expected message field, got: %s", string(body))
		}
		return nil
	}

	// Expect resource_exhausted error
	code, _ := jsonResp["code"].(string)
	if code != "resource_exhausted" {
		return fmt.Errorf("expected code resource_exhausted, got %q (body: %s)", code, string(body))
	}
	return nil
}
