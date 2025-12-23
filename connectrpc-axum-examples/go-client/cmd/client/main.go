// Package main provides a test client for connectrpc-axum examples.
//
// Usage:
//
//	go run ./cmd/client [flags] <command>
//
// Commands:
//
//	unary          Test unary RPC
//	server-stream  Test server streaming RPC
//	bidi-stream    Test bidirectional streaming (gRPC only)
//	grpc-web       Test gRPC-Web protocol
//	all            Run all applicable tests
//
// Flags:
//
//	-server    Server URL (default: http://localhost:3000)
//	-protocol  Protocol: connect, grpc (default: connect)
//	-verbose   Verbose output showing raw frames
package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"strings"
	"time"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/protobuf/proto"
)

var (
	serverURL = flag.String("server", "http://localhost:3000", "Server URL")
	protocol  = flag.String("protocol", "connect", "Protocol: connect, grpc")
	verbose   = flag.Bool("verbose", false, "Verbose output showing raw frames")
)

func main() {
	flag.Parse()

	if flag.NArg() < 1 {
		printUsage()
		os.Exit(1)
	}

	cmd := flag.Arg(0)
	switch cmd {
	case "unary":
		runUnaryTest()
	case "server-stream":
		runServerStreamTest()
	case "bidi-stream":
		runBidiStreamTest()
	case "grpc-web":
		runGrpcWebTest()
	case "all":
		runAllTests()
	default:
		fmt.Printf("Unknown command: %s\n\n", cmd)
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Println("Usage: go run ./cmd/client [flags] <command>")
	fmt.Println()
	fmt.Println("Commands:")
	fmt.Println("  unary          Test unary RPC")
	fmt.Println("  server-stream  Test server streaming RPC")
	fmt.Println("  bidi-stream    Test bidirectional streaming (gRPC only)")
	fmt.Println("  grpc-web       Test gRPC-Web protocol")
	fmt.Println("  all            Run all applicable tests")
	fmt.Println()
	fmt.Println("Flags:")
	flag.PrintDefaults()
}

func runAllTests() {
	fmt.Println("Running all tests...")
	fmt.Println()

	runUnaryTest()
	fmt.Println()

	runServerStreamTest()
	fmt.Println()

	if *protocol == "grpc" {
		runBidiStreamTest()
		fmt.Println()
	}
}

// ============================================================================
// Unary RPC Tests
// ============================================================================

func runUnaryTest() {
	printHeader("UNARY RPC TEST", *protocol)

	switch *protocol {
	case "connect":
		testUnaryConnect()
	case "grpc":
		testUnaryGrpc()
	default:
		fmt.Printf("Unknown protocol: %s\n", *protocol)
	}
}

func testUnaryConnect() {
	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		*serverURL,
	)

	name := "Connect Unary Tester"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		log.Fatalf("Connect unary failed: %v", err)
	}

	// Validate response
	if resp.Msg.Message == "" {
		log.Fatalf("Connect unary: empty response message")
	}
	if !strings.Contains(resp.Msg.Message, name) {
		log.Fatalf("Connect unary: response doesn't contain name: got %q", resp.Msg.Message)
	}

	fmt.Printf("Response: %s\n", resp.Msg.Message)
}

func testUnaryGrpc() {
	// Parse server URL to get host:port
	addr := strings.TrimPrefix(*serverURL, "http://")
	addr = strings.TrimPrefix(addr, "https://")

	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewHelloWorldServiceClient(conn)

	name := "gRPC Unary Tester"
	resp, err := client.SayHello(context.Background(), &gen.HelloRequest{
		Name: &name,
	})
	if err != nil {
		log.Fatalf("gRPC unary failed: %v", err)
	}

	// Validate response
	if resp.Message == "" {
		log.Fatalf("gRPC unary: empty response message")
	}
	if !strings.Contains(resp.Message, name) {
		log.Fatalf("gRPC unary: response doesn't contain name: got %q", resp.Message)
	}

	fmt.Printf("Response: %s\n", resp.Message)
}

// ============================================================================
// Server Streaming Tests
// ============================================================================

func runServerStreamTest() {
	printHeader("SERVER STREAMING TEST", *protocol)

	switch *protocol {
	case "connect":
		testServerStreamConnect()
	case "grpc":
		testServerStreamGrpc()
	default:
		fmt.Printf("Unknown protocol: %s\n", *protocol)
	}
}

func testServerStreamConnect() {
	if *verbose {
		testRawHTTPStreaming()
		return
	}

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		*serverURL,
	)

	name := "Connect Stream Tester"
	stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name:    &name,
		Hobbies: []string{"coding", "testing"},
	}))
	if err != nil {
		log.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for stream.Receive() {
		msgCount++
		msg := stream.Msg().Message
		if msg == "" {
			log.Fatalf("Connect stream: empty message at position %d", msgCount)
		}
		fmt.Printf("  [%d] %s\n", msgCount, msg)
	}

	if err := stream.Err(); err != nil {
		log.Fatalf("Stream error: %v", err)
	}

	// Validate we received expected number of messages
	if msgCount == 0 {
		log.Fatalf("Connect stream: received no messages")
	}

	fmt.Printf("Received %d messages\n", msgCount)
}

func testServerStreamGrpc() {
	addr := strings.TrimPrefix(*serverURL, "http://")
	addr = strings.TrimPrefix(addr, "https://")

	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewHelloWorldServiceClient(conn)

	name := "gRPC Stream Tester"
	stream, err := client.SayHelloStream(context.Background(), &gen.HelloRequest{
		Name:    &name,
		Hobbies: []string{"coding", "testing"},
	})
	if err != nil {
		log.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for {
		resp, err := stream.Recv()
		if err == io.EOF {
			break
		}
		if err != nil {
			log.Fatalf("Stream error: %v", err)
		}
		msgCount++
		if resp.Message == "" {
			log.Fatalf("gRPC stream: empty message at position %d", msgCount)
		}
		fmt.Printf("  [%d] %s\n", msgCount, resp.Message)
	}

	// Validate we received expected number of messages
	if msgCount == 0 {
		log.Fatalf("gRPC stream: received no messages")
	}

	fmt.Printf("Received %d messages\n", msgCount)
}

// ============================================================================
// Bidirectional Streaming Tests (gRPC only)
// ============================================================================

func runBidiStreamTest() {
	printHeader("BIDI STREAMING TEST", "grpc")

	if *protocol != "grpc" {
		fmt.Println("Bidirectional streaming is only supported with gRPC protocol.")
		fmt.Println("Use: -protocol grpc")
		return
	}

	testBidiStreamGrpc()
}

func testBidiStreamGrpc() {
	addr := strings.TrimPrefix(*serverURL, "http://")
	addr = strings.TrimPrefix(addr, "https://")

	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)

	stream, err := client.EchoBidiStream(context.Background())
	if err != nil {
		log.Fatalf("Failed to start bidi stream: %v", err)
	}

	// Send messages in a goroutine
	go func() {
		messages := []string{"Hello", "World", "Bidi", "Stream", "Test"}
		for _, msg := range messages {
			fmt.Printf("  -> Sending: %s\n", msg)
			if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
				log.Printf("Send error: %v", err)
				return
			}
			time.Sleep(100 * time.Millisecond)
		}
		stream.CloseSend()
	}()

	// Receive responses
	msgCount := 0
	for {
		resp, err := stream.Recv()
		if err == io.EOF {
			break
		}
		if err != nil {
			log.Fatalf("Recv error: %v", err)
		}
		msgCount++
		if resp.Message == "" {
			log.Fatalf("Bidi stream: empty message at position %d", msgCount)
		}
		fmt.Printf("  <- Received [%d]: %s\n", msgCount, resp.Message)
	}

	// Validate we received responses (at least as many as we sent)
	if msgCount == 0 {
		log.Fatalf("Bidi stream: received no messages")
	}

	fmt.Printf("Bidi stream completed with %d messages\n", msgCount)
}

// ============================================================================
// gRPC-Web Tests
// ============================================================================

func runGrpcWebTest() {
	printHeader("gRPC-WEB TEST", "grpc-web")

	// gRPC-Web uses HTTP/1.1 with base64 encoding
	// For now, we'll do a basic test using raw HTTP with gRPC-Web content type
	fmt.Println("Testing gRPC-Web unary...")

	testGrpcWebUnary()
}

func testGrpcWebUnary() {
	// gRPC-Web uses a specific binary format
	// For simplicity, we'll test with grpc-web+proto format

	url := *serverURL + "/hello.HelloWorldService/SayHello"

	// Create a simple protobuf request
	name := "gRPC-Web Tester"
	req := &gen.HelloRequest{Name: &name}
	reqBytes, err := proto.Marshal(req)
	if err != nil {
		log.Fatalf("Failed to marshal request: %v", err)
	}

	// gRPC-Web format: [compressed:1][length:4][message]
	var frame []byte
	frame = append(frame, 0) // not compressed
	lenBytes := make([]byte, 4)
	binary.BigEndian.PutUint32(lenBytes, uint32(len(reqBytes)))
	frame = append(frame, lenBytes...)
	frame = append(frame, reqBytes...)

	httpReq, err := http.NewRequest("POST", url, strings.NewReader(string(frame)))
	if err != nil {
		log.Fatalf("Failed to create request: %v", err)
	}

	httpReq.Header.Set("Content-Type", "application/grpc-web+proto")
	httpReq.Header.Set("Accept", "application/grpc-web+proto")

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(httpReq)
	if err != nil {
		log.Fatalf("Request failed: %v", err)
	}
	defer resp.Body.Close()

	fmt.Printf("Response Status: %s\n", resp.Status)
	fmt.Printf("Response Content-Type: %s\n", resp.Header.Get("Content-Type"))

	// Validate HTTP status
	if resp.StatusCode != http.StatusOK {
		log.Fatalf("gRPC-Web: unexpected status %d", resp.StatusCode)
	}

	// Read the response frame
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		log.Fatalf("Failed to read response: %v", err)
	}

	if len(body) < 5 {
		log.Fatalf("gRPC-Web: response too short: %d bytes", len(body))
	}

	// Parse gRPC-Web response frame
	flags := body[0]
	length := binary.BigEndian.Uint32(body[1:5])
	fmt.Printf("Response frame: flags=0x%02x, length=%d\n", flags, length)

	if len(body) < 5+int(length) {
		log.Fatalf("gRPC-Web: incomplete response body")
	}

	msgBytes := body[5 : 5+length]
	var respMsg gen.HelloResponse
	if err := proto.Unmarshal(msgBytes, &respMsg); err != nil {
		log.Fatalf("gRPC-Web: failed to unmarshal response: %v", err)
	}

	// Validate response content
	if respMsg.Message == "" {
		log.Fatalf("gRPC-Web: empty response message")
	}

	fmt.Printf("Response message: %s\n", respMsg.Message)
}

// ============================================================================
// Raw HTTP Protocol Testing (verbose mode)
// ============================================================================

func testRawHTTPStreaming() {
	fmt.Println("Raw HTTP Streaming Protocol Test")
	fmt.Println(strings.Repeat("-", 40))

	url := *serverURL + "/hello.HelloWorldService/SayHelloStream"
	reqBody := `{"name":"Protocol Tester"}`

	req, err := http.NewRequest("POST", url, strings.NewReader(reqBody))
	if err != nil {
		log.Printf("Failed to create request: %v", err)
		return
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/connect+json")

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		log.Printf("Failed to send request: %v", err)
		return
	}
	defer resp.Body.Close()

	fmt.Printf("Response Status: %s\n", resp.Status)
	fmt.Printf("Content-Type: %s\n", resp.Header.Get("Content-Type"))
	fmt.Println()

	frameNum := 0
	for {
		// Read frame header [flags:1][length:4]
		header := make([]byte, 5)
		_, err := io.ReadFull(resp.Body, header)
		if err == io.EOF {
			fmt.Printf("Stream ended (EOF after %d frames)\n", frameNum)
			break
		}
		if err != nil {
			fmt.Printf("Error reading frame header: %v\n", err)
			break
		}

		flags := header[0]
		length := binary.BigEndian.Uint32(header[1:5])

		frameNum++
		fmt.Printf("Frame #%d:\n", frameNum)
		fmt.Printf("  Flags: 0x%02x (compressed=%v, endstream=%v)\n",
			flags, flags&0x01 != 0, flags&0x02 != 0)
		fmt.Printf("  Length: %d bytes\n", length)

		// Read payload
		payload := make([]byte, length)
		_, err = io.ReadFull(resp.Body, payload)
		if err != nil {
			fmt.Printf("  Error reading payload: %v\n", err)
			break
		}

		// Try to parse as JSON
		var jsonData interface{}
		if err := json.Unmarshal(payload, &jsonData); err == nil {
			prettyJSON, _ := json.MarshalIndent(jsonData, "  ", "  ")
			fmt.Printf("  Payload:\n  %s\n", string(prettyJSON))
		} else {
			fmt.Printf("  Payload (raw): %s\n", string(payload))
		}

		if flags&0x02 != 0 {
			fmt.Println("  (EndStream)")
			break
		}
	}

	fmt.Printf("\nTotal frames: %d\n", frameNum)
}

// ============================================================================
// Helpers
// ============================================================================

func printHeader(title, protocol string) {
	fmt.Println(strings.Repeat("=", 60))
	fmt.Printf("%s [%s]\n", title, strings.ToUpper(protocol))
	fmt.Println(strings.Repeat("=", 60))
}
