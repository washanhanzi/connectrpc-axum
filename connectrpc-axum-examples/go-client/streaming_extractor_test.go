package main

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestStreamingExtractor tests that streaming handlers (server, client)
// can use axum extractors and state just like unary handlers.
// This validates the unified ConnectHandlerWrapper with extractor support.

func TestStreamingExtractorServerStream(t *testing.T) {
	s := startServer(t, "streaming-extractor", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	t.Run("without header should fail with UNAUTHENTICATED", func(t *testing.T) {
		name := "Alice"
		stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))
		if err != nil {
			// Error on connection - check if it's UNAUTHENTICATED
			connectErr := new(connect.Error)
			if errors.As(err, &connectErr) && connectErr.Code() == connect.CodeUnauthenticated {
				t.Logf("Got expected error on connection: %v", connectErr.Message())
				return
			}
			t.Fatalf("Unexpected error: %v", err)
		}
		defer stream.Close()

		// Try to receive - should get error
		if stream.Receive() {
			t.Fatal("Expected error, but received message")
		}

		err = stream.Err()
		if err == nil {
			t.Fatal("Expected error when x-user-id header is missing")
		}

		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}

		if connectErr.Code() != connect.CodeUnauthenticated {
			t.Fatalf("Expected CodeUnauthenticated, got: %v", connectErr.Code())
		}

		t.Logf("Got expected error: %v", connectErr.Message())
	})

	t.Run("with header should succeed and include state info", func(t *testing.T) {
		name := "Alice"
		req := connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		})
		req.Header().Set("x-user-id", "user123")

		stream, err := client.SayHelloStream(context.Background(), req)
		if err != nil {
			t.Fatalf("Failed to start stream: %v", err)
		}
		defer stream.Close()

		var messages []string
		for stream.Receive() {
			msg := stream.Msg()
			messages = append(messages, msg.Message)
			t.Logf("Received: %s", msg.Message)
		}

		if err := stream.Err(); err != nil {
			t.Fatalf("Stream error: %v", err)
		}

		if len(messages) < 3 {
			t.Fatalf("Expected at least 3 messages, got %d", len(messages))
		}

		// First message should contain state info
		if !strings.Contains(messages[0], "user123") {
			t.Fatalf("First message should contain user ID: %q", messages[0])
		}
		if !strings.Contains(messages[0], "request #") {
			t.Fatalf("First message should contain request number: %q", messages[0])
		}
	})
}

func TestStreamingExtractorClientStream(t *testing.T) {
	s := startServer(t, "streaming-extractor", "")
	defer s.stop()

	// Use raw HTTP for client streaming since it's easier to control headers
	t.Run("without header should fail with UNAUTHENTICATED", func(t *testing.T) {
		// Build streaming request body
		frame1 := buildFrame(0x00, []byte(`{"message":"Hello"}`))
		frame2 := buildFrame(0x00, []byte(`{"message":"World"}`))
		endFrame := buildFrame(0x02, []byte("{}"))

		var reqBody []byte
		reqBody = append(reqBody, frame1...)
		reqBody = append(reqBody, frame2...)
		reqBody = append(reqBody, endFrame...)

		req, err := http.NewRequest(
			"POST",
			serverURL+"/echo.EchoService/EchoClientStream",
			strings.NewReader(string(reqBody)),
		)
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/connect+json")
		req.Header.Set("Connect-Protocol-Version", "1")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		// Should return HTTP 200 with error in body (streaming protocol)
		// Or could return HTTP error directly
		if resp.StatusCode == http.StatusOK {
			// Parse streaming response to find error
			body, _ := io.ReadAll(resp.Body)
			frames, err := parseFrames(body)
			if err != nil {
				t.Fatalf("Failed to parse response frames: %v", err)
			}

			// Look for error in end stream frame
			for _, frame := range frames {
				if frame.Flags == 0x02 { // EndStream
					if strings.Contains(string(frame.Payload), "UNAUTHENTICATED") ||
						strings.Contains(string(frame.Payload), "unauthenticated") {
						t.Log("Got expected UNAUTHENTICATED error in EndStream frame")
						return
					}
				}
			}
			t.Fatalf("Expected UNAUTHENTICATED error in response")
		} else if resp.StatusCode == http.StatusUnauthorized {
			t.Log("Got expected HTTP 401 Unauthorized")
		} else {
			t.Fatalf("Expected 401 or streaming error, got status: %d", resp.StatusCode)
		}
	})

	t.Run("with header should succeed and include state info", func(t *testing.T) {
		// Build streaming request body with 3 messages
		frame1 := buildFrame(0x00, []byte(`{"message":"Hello"}`))
		frame2 := buildFrame(0x00, []byte(`{"message":"World"}`))
		frame3 := buildFrame(0x00, []byte(`{"message":"Test"}`))
		endFrame := buildFrame(0x02, []byte("{}"))

		var reqBody []byte
		reqBody = append(reqBody, frame1...)
		reqBody = append(reqBody, frame2...)
		reqBody = append(reqBody, frame3...)
		reqBody = append(reqBody, endFrame...)

		req, err := http.NewRequest(
			"POST",
			serverURL+"/echo.EchoService/EchoClientStream",
			strings.NewReader(string(reqBody)),
		)
		if err != nil {
			t.Fatalf("Failed to create request: %v", err)
		}
		req.Header.Set("Content-Type", "application/connect+json")
		req.Header.Set("Connect-Protocol-Version", "1")
		req.Header.Set("x-user-id", "user456")

		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("Request failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			t.Fatalf("Expected 200 OK, got: %d", resp.StatusCode)
		}

		body, _ := io.ReadAll(resp.Body)
		frames, err := parseFrames(body)
		if err != nil {
			t.Fatalf("Failed to parse response frames: %v", err)
		}

		// Find message frame
		for _, frame := range frames {
			if frame.Flags == 0x00 { // Message frame
				payload := string(frame.Payload)
				t.Logf("Response: %s", payload)

				if !strings.Contains(payload, "user456") {
					t.Fatalf("Response should contain user ID: %q", payload)
				}
				if !strings.Contains(payload, "request #") {
					t.Fatalf("Response should contain request number: %q", payload)
				}
				if !strings.Contains(payload, "3 messages") {
					t.Fatalf("Response should mention 3 messages: %q", payload)
				}
				return
			}
		}

		t.Fatal("No message frame found in response")
	})
}

// Note: Bidi streaming requires HTTP/2 (h2c) which is more complex to test.
// The server streaming and client streaming tests above validate that
// extractors work correctly with streaming handlers.
