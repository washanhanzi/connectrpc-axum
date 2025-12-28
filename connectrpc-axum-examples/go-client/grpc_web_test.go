package main

import (
	"encoding/binary"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/connectrpc-axum/examples/go-client/gen"
	"google.golang.org/protobuf/proto"
)

// TestGRPCWeb verifies gRPC-Web protocol support.
//
// gRPC-Web uses HTTP/1.1 with a specific binary frame format:
// - Request/Response: [flags:1][length:4][protobuf message]
// - Content-Type: application/grpc-web+proto
func TestGRPCWeb(t *testing.T) {
	s := startServer(t, "grpc-web", "tonic-web")
	defer s.stop()

	url := serverURL + "/hello.HelloWorldService/SayHello"

	// Create protobuf request
	name := "gRPC-Web Tester"
	req := &gen.HelloRequest{Name: &name}
	reqBytes, err := proto.Marshal(req)
	if err != nil {
		t.Fatalf("Failed to marshal request: %v", err)
	}

	// gRPC-Web frame format: [compressed:1][length:4][message]
	var frame []byte
	frame = append(frame, 0) // not compressed
	lenBytes := make([]byte, 4)
	binary.BigEndian.PutUint32(lenBytes, uint32(len(reqBytes)))
	frame = append(frame, lenBytes...)
	frame = append(frame, reqBytes...)

	httpReq, err := http.NewRequest("POST", url, strings.NewReader(string(frame)))
	if err != nil {
		t.Fatalf("Failed to create request: %v", err)
	}

	httpReq.Header.Set("Content-Type", "application/grpc-web+proto")
	httpReq.Header.Set("Accept", "application/grpc-web+proto")

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(httpReq)
	if err != nil {
		t.Fatalf("Request failed: %v", err)
	}
	defer resp.Body.Close()

	t.Logf("Response Status: %s", resp.Status)
	t.Logf("Content-Type: %s", resp.Header.Get("Content-Type"))

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("Expected HTTP 200, got %d", resp.StatusCode)
	}

	// Read response body
	var body []byte
	buf := make([]byte, 1024)
	for {
		n, err := resp.Body.Read(buf)
		if n > 0 {
			body = append(body, buf[:n]...)
		}
		if err != nil {
			break
		}
	}

	if len(body) < 5 {
		t.Fatalf("Response too short: %d bytes", len(body))
	}

	// Parse gRPC-Web response frame
	flags := body[0]
	length := binary.BigEndian.Uint32(body[1:5])
	t.Logf("Response frame: flags=0x%02x, length=%d", flags, length)

	if len(body) < 5+int(length) {
		t.Fatalf("Incomplete response body: have %d, need %d", len(body), 5+int(length))
	}

	msgBytes := body[5 : 5+length]
	var respMsg gen.HelloResponse
	if err := proto.Unmarshal(msgBytes, &respMsg); err != nil {
		t.Fatalf("Failed to unmarshal response: %v", err)
	}

	if respMsg.Message == "" {
		t.Fatal("Empty response message")
	}

	t.Logf("Response message: %s", respMsg.Message)
}
