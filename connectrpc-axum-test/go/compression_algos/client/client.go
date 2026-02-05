package main

import (
	"bytes"
	"compress/gzip"
	"compress/zlib"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
)

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

	type compressor func([]byte) ([]byte, error)

	tests := []struct {
		name     string
		encoding string
		compress compressor
	}{
		{"unary gzip compression", "gzip", compressGzip},
		{"unary deflate compression", "deflate", compressDeflate},
	}

	failed := false
	for _, tc := range tests {
		if err := testUnaryCompression(client, tc.name, tc.encoding, tc.compress); err != nil {
			fmt.Printf("    FAIL  %s: %v\n", tc.name, err)
			failed = true
		} else {
			fmt.Printf("    PASS  %s\n", tc.name)
		}
	}

	if failed {
		os.Exit(1)
	}
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

func compressDeflate(data []byte) ([]byte, error) {
	var buf bytes.Buffer
	writer := zlib.NewWriter(&buf)
	if _, err := writer.Write(data); err != nil {
		return nil, err
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func testUnaryCompression(client *http.Client, name, encoding string, compress func([]byte) ([]byte, error)) error {
	largeMessage := fmt.Sprintf("Test %s %s", encoding, strings.Repeat("padding ", 20))
	jsonPayload := []byte(fmt.Sprintf(`{"name":"%s"}`, largeMessage))

	compressedPayload, err := compress(jsonPayload)
	if err != nil {
		return fmt.Errorf("compressing request: %w", err)
	}

	req, err := http.NewRequest("POST", "http://localhost/hello.HelloWorldService/SayHello",
		bytes.NewReader(compressedPayload))
	if err != nil {
		return fmt.Errorf("creating request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Content-Encoding", encoding)

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("sending request: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	if resp.StatusCode != 200 {
		return fmt.Errorf("expected 200, got %d: %s", resp.StatusCode, string(body))
	}

	var result struct{ Message string `json:"message"` }
	if err := json.Unmarshal(body, &result); err != nil {
		return fmt.Errorf("parsing response: %w", err)
	}

	if result.Message == "" {
		return fmt.Errorf("empty response message")
	}

	return nil
}
