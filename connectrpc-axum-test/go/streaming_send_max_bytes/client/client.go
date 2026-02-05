package main

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
)

type testCase struct {
	name          string
	requestName   string
	expectSuccess bool
	minMessages   int
}

var testCases = []testCase{
	{"streaming with small responses succeeds", "Small", true, 2},
	{"streaming fails when response exceeds send limit", "Large", false, 0},
}

// envelopeFrame wraps a payload in the Connect streaming envelope format:
// [1 byte flags][4 bytes big-endian length][payload]
func envelopeFrame(payload []byte) []byte {
	buf := make([]byte, 5+len(payload))
	buf[0] = 0x00 // flags: data frame
	binary.BigEndian.PutUint32(buf[1:5], uint32(len(payload)))
	copy(buf[5:], payload)
	return buf
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
	url := "http://localhost/hello.HelloWorldService/SayHelloStream"

	jsonBody := fmt.Sprintf(`{"name":"%s"}`, tc.requestName)
	enveloped := envelopeFrame([]byte(jsonBody))

	req, err := http.NewRequest("POST", url, bytes.NewReader(enveloped))
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/connect+json")
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

	if tc.expectSuccess {
		if resp.StatusCode != 200 {
			return fmt.Errorf("expected HTTP 200, got %d: %s", resp.StatusCode, string(body))
		}

		// Parse streaming response frames
		cursor := body
		var messages []map[string]any

		for len(cursor) >= 5 {
			flags := cursor[0]
			payloadLen := binary.BigEndian.Uint32(cursor[1:5])
			cursor = cursor[5:]

			if uint32(len(cursor)) < payloadLen {
				break
			}

			payload := cursor[:payloadLen]
			cursor = cursor[payloadLen:]

			if flags&0x02 != 0 {
				// End-of-stream trailer - check for errors
				var endStream map[string]any
				if err := json.Unmarshal(payload, &endStream); err == nil {
					if _, hasErr := endStream["error"]; hasErr {
						return fmt.Errorf("unexpected error in EndStream: %v", endStream)
					}
				}
				break
			}

			var msg map[string]any
			if err := json.Unmarshal(payload, &msg); err != nil {
				return fmt.Errorf("invalid JSON in frame: %s", string(payload))
			}
			messages = append(messages, msg)
		}

		if len(messages) < tc.minMessages {
			return fmt.Errorf("expected at least %d messages, got %d", tc.minMessages, len(messages))
		}

		return nil
	}

	// Expect resource_exhausted error.
	// Rust server: EndStream frame with resource_exhausted error (EndStream is exempt from send_max_bytes).
	// Go server: EndStream frame may be absent because connect-go applies sendMaxBytes to
	// EndStream frames too, causing the error EndStream (~90 bytes) to exceed the 64-byte limit.
	// In that case, the body is empty (no data messages, no EndStream).
	if resp.StatusCode == 200 {
		// Empty body is acceptable (Go server behavior)
		if len(body) == 0 {
			return nil
		}

		// Parse streaming response looking for EndStream frame with error
		cursor := body
		for len(cursor) >= 5 {
			flags := cursor[0]
			payloadLen := binary.BigEndian.Uint32(cursor[1:5])
			cursor = cursor[5:]

			if uint32(len(cursor)) < payloadLen {
				break
			}

			payload := cursor[:payloadLen]
			cursor = cursor[payloadLen:]

			if flags&0x02 != 0 {
				var endStream map[string]any
				if err := json.Unmarshal(payload, &endStream); err != nil {
					return fmt.Errorf("invalid EndStream JSON: %s", string(payload))
				}

				errorObj, ok := endStream["error"].(map[string]any)
				if !ok {
					return fmt.Errorf("expected error in EndStream, got: %v", endStream)
				}

				code, ok := errorObj["code"].(string)
				if !ok || code != "resource_exhausted" {
					return fmt.Errorf("expected code resource_exhausted, got: %v", errorObj)
				}

				return nil
			}
		}

		return fmt.Errorf("expected resource_exhausted error in EndStream frame, got body: %s", string(body))
	}

	// Non-200: parse as unary-style JSON error
	var jsonResp map[string]any
	if err := json.Unmarshal(body, &jsonResp); err != nil {
		return fmt.Errorf("expected JSON error body, got: %s", string(body))
	}

	code, _ := jsonResp["code"].(string)
	if code != "resource_exhausted" {
		return fmt.Errorf("expected code resource_exhausted, got %q (body: %s)", code, string(body))
	}

	return nil
}
