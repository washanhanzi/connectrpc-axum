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
	"strings"
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
		fmt.Printf("    FAIL  server stream returns messages: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  server stream returns messages\n")
}

func runTest(client *http.Client) error {
	url := "http://localhost/hello.HelloWorldService/SayHelloStream"

	jsonBody := []byte(`{"name":"Stream Tester"}`)
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

	contentType := resp.Header.Get("Content-Type")
	if !strings.HasPrefix(contentType, "application/connect+json") {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("expected content-type application/connect+json, got: %s (body: %s)", contentType, string(body))
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	// Parse binary-framed stream: [1 byte flags][4 bytes BE length][payload]
	var messages []map[string]any
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
			// End-of-stream trailer
			break
		}

		var msg map[string]any
		if err := json.Unmarshal(payload, &msg); err != nil {
			return fmt.Errorf("invalid JSON in frame: %s", string(payload))
		}
		messages = append(messages, msg)
	}

	if len(messages) < 2 {
		return fmt.Errorf("expected at least 2 messages, got %d", len(messages))
	}

	firstMsg, ok := messages[0]["message"].(string)
	if !ok || !strings.Contains(firstMsg, "Hello") {
		return fmt.Errorf("expected first message to contain 'Hello', got: %v", messages[0]["message"])
	}

	return nil
}
