package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"strings"
	"time"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

const (
	serverURL = "http://localhost:3000"
)

// ResponseInterceptor logs raw HTTP responses to verify protocol compliance
type ResponseInterceptor struct{}

func (ri *ResponseInterceptor) WrapUnary(next connect.UnaryFunc) connect.UnaryFunc {
	return next
}

func (ri *ResponseInterceptor) WrapStreamingClient(next connect.StreamingClientFunc) connect.StreamingClientFunc {
	return func(ctx context.Context, spec connect.Spec) connect.StreamingClientConn {
		conn := next(ctx, spec)
		fmt.Printf("\n🔍 STREAMING REQUEST:\n")
		fmt.Printf("  Procedure: %s\n", spec.Procedure)
		fmt.Printf("  StreamType: %v\n", spec.StreamType)
		return &loggingStreamConn{StreamingClientConn: conn}
	}
}

func (ri *ResponseInterceptor) WrapStreamingHandler(next connect.StreamingHandlerFunc) connect.StreamingHandlerFunc {
	return next
}

// loggingStreamConn wraps a StreamingClientConn to log message details
type loggingStreamConn struct {
	connect.StreamingClientConn
	messageCount      int
	endStreamSeen     bool
	errorsEncountered []string
}

func (l *loggingStreamConn) Receive(msg any) error {
	err := l.StreamingClientConn.Receive(msg)
	l.messageCount++

	if err == nil {
		fmt.Printf("\n  ✅ Message #%d received: %+v\n", l.messageCount, msg)
	} else if err == io.EOF {
		fmt.Printf("\n  🏁 Stream ended (EOF) after %d messages\n", l.messageCount-1)
		if !l.endStreamSeen {
			fmt.Printf("  ⚠️  WARNING: No explicit EndStreamResponse was detected!\n")
		}
	} else {
		fmt.Printf("\n  ❌ Error at message #%d: %v\n", l.messageCount, err)
		l.errorsEncountered = append(l.errorsEncountered, err.Error())

		// Check if this is a ConnectError with details
		if connectErr, ok := err.(*connect.Error); ok {
			fmt.Printf("  Error Code: %s\n", connectErr.Code())
			fmt.Printf("  Error Message: %s\n", connectErr.Message())
			if len(connectErr.Details()) > 0 {
				fmt.Printf("  Error Details: %d detail(s)\n", len(connectErr.Details()))
			}
		}
	}

	return err
}

func (l *loggingStreamConn) CloseResponse() error {
	fmt.Printf("\n  🔚 CloseResponse called\n")
	if len(l.errorsEncountered) > 0 {
		fmt.Printf("  Total errors: %d\n", len(l.errorsEncountered))
	}
	return l.StreamingClientConn.CloseResponse()
}

// RawHTTPClient makes a raw HTTP request to inspect the protocol details
func testRawHTTPStreaming() {
	fmt.Println("\n" + strings.Repeat("=", 80))
	fmt.Println("🔬 RAW HTTP PROTOCOL TEST")
	fmt.Println(strings.Repeat("=", 80))

	url := serverURL + "/hello.HelloWorldService/SayHelloStream"
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

	fmt.Printf("\n📥 RESPONSE HEADERS:\n")
	fmt.Printf("  Status: %s\n", resp.Status)
	fmt.Printf("  Content-Type: %s\n", resp.Header.Get("Content-Type"))

	fmt.Printf("\n📦 RESPONSE FRAMES:\n")

	frameNum := 0
	for {
		// Read frame header [flags:1][length:4]
		header := make([]byte, 5)
		_, err := io.ReadFull(resp.Body, header)
		if err == io.EOF {
			fmt.Printf("\n  ✅ Stream ended cleanly (EOF after %d frames)\n", frameNum)
			break
		}
		if err != nil {
			fmt.Printf("\n  ❌ Error reading frame header: %v\n", err)
			break
		}

		flags := header[0]
		length := binary.BigEndian.Uint32(header[1:5])

		frameNum++
		fmt.Printf("\n  Frame #%d:\n", frameNum)
		fmt.Printf("    Flags: 0b%08b (0x%02x)\n", flags, flags)
		fmt.Printf("    - Compressed: %v\n", flags&0x01 != 0)
		fmt.Printf("    - EndStream: %v\n", flags&0x02 != 0)
		fmt.Printf("    Length: %d bytes\n", length)

		// Read payload
		payload := make([]byte, length)
		_, err = io.ReadFull(resp.Body, payload)
		if err != nil {
			fmt.Printf("    ❌ Error reading payload: %v\n", err)
			break
		}

		// Try to parse as JSON
		var jsonData interface{}
		if err := json.Unmarshal(payload, &jsonData); err == nil {
			prettyJSON, _ := json.MarshalIndent(jsonData, "    ", "  ")
			fmt.Printf("    Payload (JSON):\n    %s\n", string(prettyJSON))

			// Check if this is an EndStreamResponse
			if jsonMap, ok := jsonData.(map[string]interface{}); ok {
				if _, hasError := jsonMap["error"]; hasError {
					fmt.Printf("    ⚠️  This is an EndStreamResponse with ERROR\n")
				} else if _, hasMetadata := jsonMap["metadata"]; hasMetadata {
					fmt.Printf("    ✅ This is an EndStreamResponse with metadata\n")
				} else if len(jsonMap) == 0 {
					fmt.Printf("    ✅ This is an empty EndStreamResponse (success)\n")
				}
			}
		} else {
			fmt.Printf("    Payload (raw): %s\n", string(payload))
		}

		// If this was an EndStream frame, there should be no more data
		if flags&0x02 != 0 {
			fmt.Printf("    ✅ EndStream flag detected - stream should end\n")
			break
		}
	}

	fmt.Printf("\n  Total frames received: %d\n", frameNum)
}

// Test using official Connect client
func testConnectClient() {
	fmt.Println("\n" + strings.Repeat("=", 80))
	fmt.Println("🔌 CONNECT CLIENT TEST")
	fmt.Println(strings.Repeat("=", 80))

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
		connect.WithInterceptors(&ResponseInterceptor{}),
	)

	ctx := context.Background()

	name := "Connect Client Tester"
	fmt.Println("\n📤 Calling SayHelloStream...")
	stream, err := client.SayHelloStream(ctx, connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))

	if err != nil {
		log.Fatalf("Failed to start stream: %v", err)
	}

	fmt.Println("\n📥 Receiving messages...")
	for stream.Receive() {
		msg := stream.Msg()
		fmt.Printf("  📨 Received: %s\n", msg.Message)
	}

	if err := stream.Err(); err != nil {
		fmt.Printf("\n  ❌ Stream error: %v\n", err)
	}

	if err := stream.Close(); err != nil {
		fmt.Printf("\n  ❌ Close error: %v\n", err)
	}
}

func main() {
	fmt.Println("🧪 Connect Protocol Streaming Verification Tool")
	fmt.Println("Testing server at:", serverURL)
	fmt.Println()

	// First, test with raw HTTP to see the actual protocol frames
	testRawHTTPStreaming()

	// Wait a bit between tests
	time.Sleep(1 * time.Second)

	// Then test with the official Connect client
	testConnectClient()

	fmt.Println("\n" + strings.Repeat("=", 80))
	fmt.Println("✅ Tests completed")
	fmt.Println(strings.Repeat("=", 80))
}
