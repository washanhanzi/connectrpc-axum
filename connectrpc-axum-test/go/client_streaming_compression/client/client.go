package main

import (
	"bytes"
	"compress/gzip"
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

func decompressGzip(data []byte) ([]byte, error) {
	reader, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	defer reader.Close()
	return io.ReadAll(reader)
}

func compressGzip(data []byte) ([]byte, error) {
	var buf bytes.Buffer
	writer := gzip.NewWriter(&buf)
	if _, err := writer.Write(data); err != nil {
		return nil, err
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
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
		fmt.Printf("    FAIL  compressed client stream frames are decompressed: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  compressed client stream frames are decompressed\n")
}

func runTest(client *http.Client) error {
	// Frame 1: compressed (connect-go requires all frames compressed when encoding is set)
	compressed1, err := compressGzip([]byte(`{"message":"Hello from Alice"}`))
	if err != nil {
		return fmt.Errorf("compressing frame 1: %w", err)
	}
	frame1 := envelopeFrame(0x01, compressed1)

	// Frame 2: compressed
	compressed2, err := compressGzip([]byte(`{"message":"Hello from Bob via compressed frame"}`))
	if err != nil {
		return fmt.Errorf("compressing frame 2: %w", err)
	}
	frame2 := envelopeFrame(0x01, compressed2)

	// Frame 3: compressed
	compressed3, err := compressGzip([]byte(`{"message":"Hello from Charlie also compressed"}`))
	if err != nil {
		return fmt.Errorf("compressing frame 3: %w", err)
	}
	frame3 := envelopeFrame(0x01, compressed3)

	endFrame := envelopeFrame(0x02, []byte("{}"))

	var reqBody bytes.Buffer
	reqBody.Write(frame1)
	reqBody.Write(frame2)
	reqBody.Write(frame3)
	reqBody.Write(endFrame)

	req, err := http.NewRequest("POST", "http://localhost/echo.EchoService/EchoClientStream", &reqBody)
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Connect-Content-Encoding", "gzip")

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	if resp.StatusCode != 200 {
		return fmt.Errorf("expected 200, got %d: %s", resp.StatusCode, string(body))
	}

	// Parse response (may be streaming or plain JSON)
	var message string
	if len(body) >= 5 && (body[0] == 0x00 || body[0] == 0x01) {
		cursor := body
		for len(cursor) >= 5 {
			flags := cursor[0]
			payloadLen := binary.BigEndian.Uint32(cursor[1:5])
			cursor = cursor[5:]
			if uint32(len(cursor)) < payloadLen { break }
			payload := cursor[:payloadLen]
			cursor = cursor[payloadLen:]
			if flags&0x02 != 0 { break }
			jsonPayload := payload
			if flags&0x01 != 0 {
				decompressed, err := decompressGzip(payload)
				if err != nil {
					return fmt.Errorf("failed to decompress response frame: %w", err)
				}
				jsonPayload = decompressed
			}
			var msg struct{ Message string `json:"message"` }
			if err := json.Unmarshal(jsonPayload, &msg); err == nil {
				message = msg.Message
			}
		}
	} else {
		var msg struct{ Message string `json:"message"` }
		if err := json.Unmarshal(body, &msg); err != nil {
			return fmt.Errorf("failed to parse response: %w", err)
		}
		message = msg.Message
	}

	if !strings.Contains(message, "3 messages") {
		return fmt.Errorf("expected '3 messages' in response, got: %s", message)
	}
	for _, name := range []string{"Alice", "Bob", "Charlie"} {
		if !strings.Contains(message, name) {
			return fmt.Errorf("expected %q in response, got: %s", name, message)
		}
	}

	return nil
}
