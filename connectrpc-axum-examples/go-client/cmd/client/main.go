// Package main provides a test client for connectrpc-axum examples.
//
// Usage:
//
//	go run ./cmd/client [flags] <command>
//
// Commands:
//
//	unary           Test unary RPC
//	server-stream   Test server streaming RPC
//	client-stream   Test client streaming RPC (Connect protocol)
//	bidi-stream     Test bidirectional streaming (gRPC only)
//	connect-bidi    Test bidirectional streaming (Connect protocol)
//	grpc-web        Test gRPC-Web protocol
//	stream-error    Test streaming error handling (bug reproduction)
//	protocol-version Test Connect-Protocol-Version header validation
//	all             Run all applicable tests
//
// Flags:
//
//	-server    Server URL (default: http://localhost:3000)
//	-protocol  Protocol: connect, grpc (default: connect)
//	-verbose   Verbose output showing raw frames
package main

import (
	"context"
	"crypto/tls"
	"encoding/binary"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
	"time"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"golang.org/x/net/http2"
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
	case "client-stream":
		runClientStreamTest()
	case "bidi-stream":
		runBidiStreamTest()
	case "connect-bidi":
		runConnectBidiStreamTest()
	case "grpc-web":
		runGrpcWebTest()
	case "stream-error":
		runStreamErrorTest()
	case "protocol-version":
		runProtocolVersionTest()
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
	fmt.Println("  unary           Test unary RPC")
	fmt.Println("  server-stream   Test server streaming RPC")
	fmt.Println("  client-stream   Test client streaming RPC (Connect protocol)")
	fmt.Println("  bidi-stream     Test bidirectional streaming (gRPC only)")
	fmt.Println("  connect-bidi    Test bidirectional streaming (Connect protocol)")
	fmt.Println("  grpc-web        Test gRPC-Web protocol")
	fmt.Println("  stream-error    Test streaming error handling (bug reproduction)")
	fmt.Println("  protocol-version Test Connect-Protocol-Version header validation")
	fmt.Println("  all             Run all applicable tests")
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
// Client Streaming Tests (Connect protocol)
// ============================================================================

func runClientStreamTest() {
	printHeader("CLIENT STREAMING TEST", *protocol)

	switch *protocol {
	case "connect":
		testClientStreamConnect()
	case "grpc":
		testClientStreamGrpc()
	default:
		fmt.Printf("Unknown protocol: %s\n", *protocol)
	}
}

func testClientStreamConnect() {
	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		*serverURL,
	)

	stream := client.EchoClientStream(context.Background())

	// Send multiple messages
	messages := []string{"Hello", "World", "from", "Client", "Stream"}
	for _, msg := range messages {
		fmt.Printf("  -> Sending: %s\n", msg)
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			log.Fatalf("Send error: %v", err)
		}
		time.Sleep(100 * time.Millisecond)
	}

	// Close send and get response
	resp, err := stream.CloseAndReceive()
	if err != nil {
		log.Fatalf("CloseAndReceive error: %v", err)
	}

	fmt.Printf("  <- Response: %s\n", resp.Msg.Message)

	// Validate response contains expected info
	if resp.Msg.Message == "" {
		log.Fatalf("Client stream: empty response message")
	}
	if !strings.Contains(resp.Msg.Message, fmt.Sprintf("%d", len(messages))) {
		log.Printf("Warning: Response may not reflect all %d messages: %s", len(messages), resp.Msg.Message)
	}

	fmt.Println("Client streaming test passed!")
}

func testClientStreamGrpc() {
	addr := strings.TrimPrefix(*serverURL, "http://")
	addr = strings.TrimPrefix(addr, "https://")

	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)

	stream, err := client.EchoClientStream(context.Background())
	if err != nil {
		log.Fatalf("Failed to start client stream: %v", err)
	}

	// Send multiple messages
	messages := []string{"Hello", "World", "from", "gRPC", "Client", "Stream"}
	for _, msg := range messages {
		fmt.Printf("  -> Sending: %s\n", msg)
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			log.Fatalf("Send error: %v", err)
		}
		time.Sleep(100 * time.Millisecond)
	}

	// Close and receive response
	resp, err := stream.CloseAndRecv()
	if err != nil {
		log.Fatalf("CloseAndRecv error: %v", err)
	}

	fmt.Printf("  <- Response: %s\n", resp.Message)

	// Validate response
	if resp.Message == "" {
		log.Fatalf("gRPC client stream: empty response message")
	}

	fmt.Println("gRPC client streaming test passed!")
}

// ============================================================================
// Connect Bidirectional Streaming Tests
// ============================================================================

func runConnectBidiStreamTest() {
	printHeader("CONNECT BIDI STREAMING TEST", "connect")

	testConnectBidiStream()
}

func testConnectBidiStream() {
	// Create HTTP/2 client with h2c (cleartext) support for bidi streaming
	h2cClient := &http.Client{
		Transport: &http2.Transport{
			AllowHTTP: true,
			DialTLSContext: func(ctx context.Context, network, addr string, _ *tls.Config) (net.Conn, error) {
				// Use regular TCP connection (not TLS) for h2c
				var d net.Dialer
				return d.DialContext(ctx, network, addr)
			},
		},
	}

	client := genconnect.NewEchoServiceClient(
		h2cClient,
		*serverURL,
	)

	stream := client.EchoBidiStream(context.Background())

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
		if err := stream.CloseRequest(); err != nil {
			log.Printf("CloseRequest error: %v", err)
		}
	}()

	// Receive responses
	msgCount := 0
	for {
		resp, err := stream.Receive()
		if err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			// Check if we already received messages - if so, this might just be stream termination
			if msgCount > 0 && strings.Contains(err.Error(), "EOF") {
				fmt.Printf("  (Stream ended with: %v)\n", err)
				break
			}
			log.Fatalf("Receive error: %v", err)
		}
		msgCount++
		if resp.Message == "" {
			log.Fatalf("Bidi stream: empty message at position %d", msgCount)
		}
		fmt.Printf("  <- Received [%d]: %s\n", msgCount, resp.Message)
	}

	if err := stream.CloseResponse(); err != nil {
		// Ignore close errors after successful message receipt
		if msgCount == 0 {
			log.Printf("CloseResponse error: %v", err)
		}
	}

	// Validate we received responses
	if msgCount == 0 {
		log.Fatalf("Connect bidi stream: received no messages")
	}

	fmt.Printf("Connect bidi stream completed with %d messages\n", msgCount)
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
// Streaming Error Tests
// ============================================================================

func runStreamErrorTest() {
	printHeader("STREAMING ERROR HANDLING TEST", "connect")

	fmt.Println("Testing that streaming handlers returning errors BEFORE the stream starts")
	fmt.Println("produce proper Connect streaming responses:")
	fmt.Println("  - HTTP 200 with Content-Type: application/connect+json")
	fmt.Println("  - Error in EndStream frame (flags=0x02)")
	fmt.Println()

	// Test cases that trigger early errors
	testCases := []struct {
		name        string
		expectedErr string
	}{
		{"unauthorized", "permission_denied"},
		{"invalid", "invalid_argument"},
		{"notfound", "not_found"},
	}

	allPassed := true
	for _, tc := range testCases {
		fmt.Printf("\n--- Testing name='%s' (expecting %s) ---\n", tc.name, tc.expectedErr)
		if !testStreamingErrorConnect(tc.name, tc.expectedErr) {
			allPassed = false
		}
	}

	// Also test normal case
	fmt.Printf("\n--- Testing name='Alice' (normal stream, should succeed) ---\n")
	if !testStreamingErrorConnect("Alice", "") {
		allPassed = false
	}

	if !allPassed {
		log.Fatal("Stream error tests failed")
	}
	fmt.Println("\nAll stream error tests passed!")
}

func testStreamingErrorConnect(name string, expectedErrCode string) bool {
	// Use raw HTTP to see exactly what the server returns
	url := *serverURL + "/hello.HelloWorldService/SayHelloStream"
	reqBody := fmt.Sprintf(`{"name":"%s"}`, name)

	req, err := http.NewRequest("POST", url, strings.NewReader(reqBody))
	if err != nil {
		log.Printf("Failed to create request: %v", err)
		return false
	}

	// For server-streaming, the request is unary (application/json)
	// but the response is streaming (application/connect+json)
	req.Header.Set("Content-Type", "application/json")

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		log.Printf("Failed to send request: %v", err)
		return false
	}
	defer resp.Body.Close()

	fmt.Printf("HTTP Status: %d %s\n", resp.StatusCode, resp.Status)
	fmt.Printf("Content-Type: %s\n", resp.Header.Get("Content-Type"))

	// Read the response body
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		log.Printf("Failed to read response: %v", err)
		return false
	}

	contentType := resp.Header.Get("Content-Type")

	// Check HTTP status - must be 200 for streaming
	if resp.StatusCode != http.StatusOK {
		fmt.Printf("FAIL: Server returned non-200 status for streaming endpoint.\n")
		fmt.Printf("Connect protocol requires HTTP 200 for streaming responses.\n")
		return false
	}

	// Check content type - must be application/connect+json
	if !strings.HasPrefix(contentType, "application/connect+json") {
		fmt.Printf("FAIL: Expected Content-Type: application/connect+json, got: %s\n", contentType)
		return false
	}

	// Parse Connect streaming frames and validate
	fmt.Println("Response frames:")
	return parseConnectFramesAndValidate(body, expectedErrCode)
}

func parseConnectFrames(body []byte) {
	parseConnectFramesAndValidate(body, "")
}

func parseConnectFramesAndValidate(body []byte, expectedErrCode string) bool {
	offset := 0
	frameNum := 0
	hasEndStream := false
	foundExpectedError := false
	hasMessages := false

	for offset < len(body) {
		if len(body)-offset < 5 {
			fmt.Printf("  (incomplete frame header, %d bytes remaining)\n", len(body)-offset)
			break
		}

		flags := body[offset]
		length := binary.BigEndian.Uint32(body[offset+1 : offset+5])
		offset += 5

		frameNum++
		isEndStream := flags&0x02 != 0

		if int(length) > len(body)-offset {
			fmt.Printf("  Frame #%d: flags=0x%02x, length=%d (TRUNCATED)\n", frameNum, flags, length)
			break
		}

		payload := body[offset : offset+int(length)]
		offset += int(length)

		fmt.Printf("  Frame #%d: flags=0x%02x (endstream=%v), length=%d\n",
			frameNum, flags, isEndStream, length)

		// Parse JSON payload
		var jsonData map[string]interface{}
		if err := json.Unmarshal(payload, &jsonData); err == nil {
			// Check if it's an error frame
			if errData, ok := jsonData["error"]; ok {
				errMap, _ := errData.(map[string]interface{})
				errCode, _ := errMap["code"].(string)
				prettyJSON, _ := json.MarshalIndent(errData, "    ", "  ")
				fmt.Printf("    ERROR: %s\n", string(prettyJSON))

				// Check if this is the expected error
				if expectedErrCode != "" && errCode == expectedErrCode {
					foundExpectedError = true
				}
			} else if msg, ok := jsonData["message"]; ok {
				fmt.Printf("    message: %v\n", msg)
				hasMessages = true
			} else {
				prettyJSON, _ := json.MarshalIndent(jsonData, "    ", "  ")
				fmt.Printf("    %s\n", string(prettyJSON))
			}
		} else {
			fmt.Printf("    (raw): %s\n", string(payload))
		}

		if isEndStream {
			fmt.Println("  (EndStream)")
			hasEndStream = true
			break
		}
	}

	// Validate based on expected error
	if expectedErrCode != "" {
		// Expecting an error response
		if !foundExpectedError {
			fmt.Printf("FAIL: Expected error code '%s' not found\n", expectedErrCode)
			return false
		}
		if !hasEndStream {
			fmt.Println("FAIL: Missing EndStream frame")
			return false
		}
		return true
	}

	// Expecting a successful stream
	if !hasMessages {
		fmt.Println("FAIL: No messages received in stream")
		return false
	}
	if !hasEndStream {
		fmt.Println("FAIL: Missing EndStream frame")
		return false
	}
	return true
}

// testStreamingErrorWithConnectClient tests using the official Connect client
// to show how the bug manifests in real client code
func testStreamingErrorWithConnectClient(name string) {
	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		*serverURL,
	)

	stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		// When the bug occurs, the Connect client may fail here with a confusing error
		// because it receives application/json instead of application/connect+json
		fmt.Printf("Stream creation error: %v\n", err)

		// Check if it's a Connect error we can inspect
		var connectErr *connect.Error
		if ok := errors.As(err, &connectErr); ok {
			fmt.Printf("  Connect error code: %s\n", connectErr.Code())
			fmt.Printf("  Connect error message: %s\n", connectErr.Message())
		} else {
			fmt.Printf("  (Error is not a *connect.Error - this indicates protocol failure)\n")
		}
		return
	}

	// Try to read from stream
	msgCount := 0
	for stream.Receive() {
		msgCount++
		fmt.Printf("  [%d] %s\n", msgCount, stream.Msg().Message)
	}

	if err := stream.Err(); err != nil {
		fmt.Printf("Stream error: %v\n", err)
		var connectErr *connect.Error
		if ok := errors.As(err, &connectErr); ok {
			fmt.Printf("  Connect error code: %s\n", connectErr.Code())
			fmt.Printf("  Connect error message: %s\n", connectErr.Message())
		}
		return
	}

	fmt.Printf("Stream completed with %d messages\n", msgCount)
}

// ============================================================================
// Protocol Version Tests
// ============================================================================

func runProtocolVersionTest() {
	printHeader("PROTOCOL VERSION HEADER TEST", "connect")

	fmt.Println("Testing Connect-Protocol-Version header validation:")
	fmt.Println("  - Server requires: Connect-Protocol-Version: 1")
	fmt.Println("  - connect-go library automatically sends this header")
	fmt.Println()

	// Test using connect-go client - should succeed because connect-go
	// automatically sends Connect-Protocol-Version: 1 header
	fmt.Println("--- Using connect-go client (sends header automatically) ---")
	testProtocolVersionWithConnectClient()

	fmt.Println("\nProtocol version test passed!")
}

func testProtocolVersionWithConnectClient() {
	// The connect-go library automatically sends Connect-Protocol-Version: 1
	// This test verifies the server correctly accepts it
	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		*serverURL,
	)

	name := "Protocol Version Tester"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		log.Fatalf("Connect client failed: %v", err)
	}

	// Validate response
	if resp.Msg.Message == "" {
		log.Fatalf("Protocol version test: empty response message")
	}
	if !strings.Contains(resp.Msg.Message, name) {
		log.Fatalf("Protocol version test: response doesn't contain name: got %q", resp.Msg.Message)
	}

	fmt.Printf("Response: %s\n", resp.Msg.Message)
	fmt.Println("PASS: connect-go client successfully called server with protocol version validation")
}

// ============================================================================
// Helpers
// ============================================================================

func printHeader(title, protocol string) {
	fmt.Println(strings.Repeat("=", 60))
	fmt.Printf("%s [%s]\n", title, strings.ToUpper(protocol))
	fmt.Println(strings.Repeat("=", 60))
}
