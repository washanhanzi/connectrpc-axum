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
	requestBody   string
	expectSuccess bool
}

var testCases = []testCase{
	{"small streaming request succeeds", `{"name":"Alice"}`, true},
	{"large streaming request fails", `{"name":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}`, false},
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

	enveloped := envelopeFrame([]byte(tc.requestBody))

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
				// End-of-stream trailer
				break
			}

			var msg map[string]any
			if err := json.Unmarshal(payload, &msg); err != nil {
				return fmt.Errorf("invalid JSON in frame: %s", string(payload))
			}
			messages = append(messages, msg)
		}

		if len(messages) == 0 {
			return fmt.Errorf("expected at least 1 message, got 0")
		}

		firstMsg, ok := messages[0]["message"].(string)
		if !ok || firstMsg == "" {
			return fmt.Errorf("expected message field in first frame, got: %v", messages[0])
		}

		return nil
	}

	// Expect resource_exhausted error.
	// The error may come as:
	// 1. An HTTP-level error with a JSON body (non-200 status)
	// 2. An EndStream frame with error inside (HTTP 200)
	if resp.StatusCode == 200 {
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
				// End-of-stream trailer frame
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
