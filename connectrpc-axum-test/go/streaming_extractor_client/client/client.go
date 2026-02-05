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

	failed := false

	if err := testWithoutApiKey(client); err != nil {
		fmt.Printf("    FAIL  client streaming without x-api-key returns unauthenticated: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  client streaming without x-api-key returns unauthenticated\n")
	}

	if err := testWithApiKey(client); err != nil {
		fmt.Printf("    FAIL  client streaming with x-api-key succeeds: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  client streaming with x-api-key succeeds\n")
	}

	if failed {
		os.Exit(1)
	}
}

func testWithoutApiKey(client *http.Client) error {
	var reqBody bytes.Buffer
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"Hello"}`)))
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"World"}`)))
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

	body, _ := io.ReadAll(resp.Body)

	if resp.StatusCode != 200 {
		var errBody map[string]any
		if err := json.Unmarshal(body, &errBody); err != nil {
			return fmt.Errorf("expected JSON error body, got: %s", string(body))
		}
		code, _ := errBody["code"].(string)
		if code != "unauthenticated" {
			return fmt.Errorf("expected 'unauthenticated', got: %v", errBody)
		}
		return nil
	}

	// Check EndStream for error
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
			var es map[string]any
			if err := json.Unmarshal(payload, &es); err == nil {
				if errObj, ok := es["error"].(map[string]any); ok {
					code, _ := errObj["code"].(string)
					if code == "unauthenticated" {
						return nil
					}
				}
			}
		}
	}

	return fmt.Errorf("expected unauthenticated error, got 200: %s", string(body))
}

func testWithApiKey(client *http.Client) error {
	var reqBody bytes.Buffer
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"Hello"}`)))
	reqBody.Write(envelopeFrame(0x00, []byte(`{"message":"World"}`)))
	reqBody.Write(envelopeFrame(0x02, []byte("{}")))

	req, err := http.NewRequest("POST", "http://localhost/echo.EchoService/EchoClientStream", &reqBody)
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}
	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("x-api-key", "test-key")

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	if resp.StatusCode != 200 {
		return fmt.Errorf("expected 200, got %d: %s", resp.StatusCode, string(body))
	}

	// Parse response
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
			return fmt.Errorf("parsing response: %w", err)
		}
		message = msg.Message
	}

	if !strings.Contains(message, "2 messages") {
		return fmt.Errorf("expected '2 messages', got: %s", message)
	}

	return nil
}
