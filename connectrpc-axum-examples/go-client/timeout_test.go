package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

// TestTimeout verifies Connect-Timeout-Ms header enforcement.
//
// The timeout server handler sleeps for 500ms before responding.
// - Requests with timeout < 500ms should get deadline_exceeded
// - Requests with timeout >= 500ms should succeed
func TestTimeout(t *testing.T) {
	s := startServer(t, "timeout", "")
	defer s.stop()

	tests := []struct {
		name        string
		timeoutMs   int // 0 means no header
		wantSuccess bool
	}{
		{"short timeout fails", 100, false},
		{"long timeout succeeds", 1000, true},
		{"no timeout succeeds", 0, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := sendTimeoutRequest(t, tt.timeoutMs)

			if tt.wantSuccess {
				if result.err != nil {
					t.Fatalf("expected success, got error: %v", result.err)
				}
				if result.message == "" {
					t.Fatal("expected non-empty message")
				}
				t.Logf("Success: %s", result.message)
			} else {
				if result.errCode != "deadline_exceeded" {
					t.Fatalf("expected deadline_exceeded, got code=%q err=%v", result.errCode, result.err)
				}
				t.Logf("Got expected error: %s", result.errCode)
			}
		})
	}
}

type timeoutResult struct {
	message string
	errCode string
	err     error
}

func sendTimeoutRequest(t *testing.T, timeoutMs int) timeoutResult {
	url := serverURL + "/hello.HelloWorldService/SayHello"

	req, err := http.NewRequest("POST", url, strings.NewReader(`{"name":"Timeout Tester"}`))
	if err != nil {
		return timeoutResult{err: err}
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")
	if timeoutMs > 0 {
		req.Header.Set("Connect-Timeout-Ms", fmt.Sprintf("%d", timeoutMs))
	}

	client := &http.Client{Timeout: 5 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return timeoutResult{err: err}
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return timeoutResult{err: err}
	}

	var jsonResp map[string]interface{}
	if err := json.Unmarshal(body, &jsonResp); err != nil {
		return timeoutResult{err: fmt.Errorf("invalid JSON: %s", string(body))}
	}

	// Check for error response
	if code, ok := jsonResp["code"].(string); ok {
		return timeoutResult{errCode: code}
	}

	// Check for success response
	if msg, ok := jsonResp["message"].(string); ok {
		return timeoutResult{message: msg}
	}

	return timeoutResult{err: fmt.Errorf("unexpected response: %s", string(body))}
}
