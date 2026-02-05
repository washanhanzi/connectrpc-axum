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

	err := runTest(client)
	if err != nil {
		fmt.Printf("    FAIL  streaming error in EndStream frame: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  streaming error in EndStream frame\n")
}

func runTest(client *http.Client) error {
	url := "http://localhost/hello.HelloWorldService/SayHelloStream"

	jsonBody := []byte(`{"name":"Error Tester"}`)
	enveloped := envelopeFrame(jsonBody)

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

	// HTTP status should be 200 for streaming errors
	if resp.StatusCode != 200 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("expected HTTP 200, got %d: %s", resp.StatusCode, string(body))
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	// Parse binary-framed stream looking for EndStream frame
	cursor := body
	foundEndStream := false
	var endStreamError map[string]any

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
			foundEndStream = true
			if err := json.Unmarshal(payload, &endStreamError); err != nil {
				return fmt.Errorf("invalid EndStream JSON: %s", string(payload))
			}
			break
		}
	}

	if !foundEndStream {
		return fmt.Errorf("expected EndStream frame in response, got body: %s", string(body))
	}

	// Validate the error in the EndStream frame
	errorObj, ok := endStreamError["error"].(map[string]any)
	if !ok {
		return fmt.Errorf("expected error field in EndStream, got: %v", endStreamError)
	}

	code, ok := errorObj["code"].(string)
	if !ok || code != "internal" {
		return fmt.Errorf("expected error code 'internal', got: %v", errorObj)
	}

	message, ok := errorObj["message"].(string)
	if !ok || message != "something went wrong" {
		return fmt.Errorf("expected error message 'something went wrong', got: %v", errorObj)
	}

	return nil
}
