package main

import (
	"context"
	"encoding/binary"
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

	failed := false

	if err := testGRPCWebUnary(socketPath); err != nil {
		fmt.Printf("    FAIL  gRPC-Web unary request is accepted: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  gRPC-Web unary request is accepted\n")
	}

	if failed {
		os.Exit(1)
	}
}

func testGRPCWebUnary(socketPath string) error {
	transport := &http.Transport{
		DialContext: func(_ context.Context, _, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		},
	}
	client := &http.Client{Transport: transport}

	// Build protobuf HelloRequest with name="Alice"
	// Field 1 (name), wire type 2 (length-delimited): tag = 0x0a
	name := []byte("Alice")
	protoBytes := []byte{0x0a, byte(len(name))}
	protoBytes = append(protoBytes, name...)

	// gRPC frame: [compressed:1][length:4][message]
	grpcFrame := make([]byte, 5+len(protoBytes))
	grpcFrame[0] = 0x00 // not compressed
	binary.BigEndian.PutUint32(grpcFrame[1:5], uint32(len(protoBytes)))
	copy(grpcFrame[5:], protoBytes)

	req, err := http.NewRequest("POST", "http://localhost/hello.HelloWorldService/SayHello", strings.NewReader(string(grpcFrame)))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/grpc-web+proto")

	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("expected HTTP 200, got %d: %s", resp.StatusCode, string(body))
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("failed to read response body: %w", err)
	}

	// Parse gRPC-Web response frame
	if len(body) < 5 {
		return fmt.Errorf("gRPC-Web response too short: %d bytes", len(body))
	}

	msgLen := binary.BigEndian.Uint32(body[1:5])
	if uint32(len(body)) < 5+msgLen {
		return fmt.Errorf("incomplete gRPC-Web response: expected %d bytes, got %d", 5+msgLen, len(body))
	}

	msgBytes := body[5 : 5+msgLen]

	// Parse protobuf HelloResponse - field 1 is message (string)
	// Look for tag 0x0a (field 1, wire type 2)
	message := ""
	for i := 0; i < len(msgBytes); {
		tag := msgBytes[i]
		i++
		if tag == 0x0a {
			fieldLen := int(msgBytes[i])
			i++
			message = string(msgBytes[i : i+fieldLen])
			break
		}
	}

	if !strings.Contains(message, "Alice") {
		return fmt.Errorf("expected message to contain 'Alice', got: %q", message)
	}
	return nil
}
