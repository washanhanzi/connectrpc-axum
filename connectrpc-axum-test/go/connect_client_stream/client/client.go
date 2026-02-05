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

func envelopeFrame(flags byte, payload []byte) []byte {
	buf := make([]byte, 5+len(payload))
	buf[0] = flags
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

	if err := runTest(client); err != nil {
		fmt.Printf("    FAIL  client stream aggregates messages: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  client stream aggregates messages\n")
}

func runTest(client *http.Client) error {
	var reqBody bytes.Buffer
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"Hello"}`)))
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"World"}`)))
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"Test"}`)))
	reqBody.Write(envelopeFrame(0x02, []byte("{}")))

	req, err := http.NewRequest("POST", "http://localhost/echo.EchoService/EchoClientStream", &reqBody)
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

	if resp.StatusCode != 200 {
		return fmt.Errorf("expected 200, got %d: %s", resp.StatusCode, string(body))
	}

	// Try streaming format first
	var message string
	if len(body) >= 5 && (body[0] == 0x00 || body[0] == 0x01) {
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
				break
			}
			if flags == 0x00 {
				var msg struct {
					Message string `json:"message"`
				}
				if err := json.Unmarshal(payload, &msg); err == nil {
					message = msg.Message
				}
			}
		}
	} else {
		var msg struct {
			Message string `json:"message"`
		}
		if err := json.Unmarshal(body, &msg); err != nil {
			return fmt.Errorf("failed to parse response: %w", err)
		}
		message = msg.Message
	}

	if !strings.Contains(message, "3 messages") {
		return fmt.Errorf("expected '3 messages' in response, got: %s", message)
	}
	for _, name := range []string{"Hello", "World", "Test"} {
		if !strings.Contains(message, name) {
			return fmt.Errorf("expected %q in response, got: %s", name, message)
		}
	}

	return nil
}
