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
)

func envelopeFrame(payload []byte) []byte {
	buf := make([]byte, 5+len(payload))
	buf[0] = 0x00
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
		fmt.Printf("    FAIL  server stream messages are compressed with gzip: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  server stream messages are compressed with gzip\n")
}

func runTest(client *http.Client) error {
	jsonPayload := []byte(`{"name":"TestUser"}`)
	reqBody := envelopeFrame(jsonPayload)

	req, err := http.NewRequest("POST", "http://localhost/hello.HelloWorldService/SayHelloStream", bytes.NewReader(reqBody))
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Connect-Accept-Encoding", "gzip")

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("expected 200, got %d: %s", resp.StatusCode, string(body))
	}

	connectEncoding := resp.Header.Get("Connect-Content-Encoding")
	if connectEncoding != "gzip" {
		return fmt.Errorf("expected Connect-Content-Encoding: gzip, got: %q", connectEncoding)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("reading body: %w", err)
	}

	cursor := body
	compressedCount := 0
	uncompressedCount := 0

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

		if flags&0x01 != 0 {
			compressedCount++
			decompressed, err := decompressGzip(payload)
			if err != nil {
				return fmt.Errorf("failed to decompress gzip frame: %w", err)
			}
			var msg map[string]any
			if err := json.Unmarshal(decompressed, &msg); err != nil {
				return fmt.Errorf("invalid JSON in decompressed frame: %w", err)
			}
		} else {
			uncompressedCount++
			var msg map[string]any
			if err := json.Unmarshal(payload, &msg); err != nil {
				return fmt.Errorf("invalid JSON in frame: %w", err)
			}
		}
	}

	_ = uncompressedCount // Go server may compress all frames

	if compressedCount == 0 {
		return fmt.Errorf("expected at least 1 compressed frame (flag 0x01), got 0")
	}

	return nil
}
