package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
)

type testCase struct {
	name string
	run  func(client *http.Client) error
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

	testCases := []testCase{
		{"health endpoint returns ok", testHealth},
		{"metrics endpoint returns text", testMetrics},
		{"SayHello RPC with plain routes mounted", testSayHello},
	}

	failures := 0
	for _, tc := range testCases {
		err := tc.run(client)
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

func testHealth(client *http.Client) error {
	resp, err := client.Get("http://localhost/health")
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("expected status 200, got %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	if string(body) != "ok" {
		return fmt.Errorf("expected body %q, got %q", "ok", string(body))
	}

	return nil
}

func testMetrics(client *http.Client) error {
	resp, err := client.Get("http://localhost/metrics")
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("expected status 200, got %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	if len(body) == 0 {
		return fmt.Errorf("expected non-empty body")
	}

	return nil
}

func testSayHello(client *http.Client) error {
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

	contentType := resp.Header.Get("Content-Type")
	if !strings.HasPrefix(contentType, "application/json") {
		return fmt.Errorf("expected content-type application/json, got: %s", contentType)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	bodyStr := string(body)
	if !strings.Contains(bodyStr, "Hello, Alice!") {
		return fmt.Errorf("expected greeting containing 'Hello, Alice!', got: %s", bodyStr)
	}

	return nil
}
