package main

import (
	"context"
	"errors"
	"net/http"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestEndStreamMetadata verifies that EndStream frames include metadata correctly:
// - Error metadata is merged into the EndStream frame's "metadata" field
// - Protocol headers (connect-*, grpc-*, content-type) are filtered
// - Custom headers are preserved
// - Values are arrays of strings (per Connect protocol spec)
func TestEndStreamMetadata(t *testing.T) {
	s := startServer(t, "endstream-metadata", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	t.Run("error with custom metadata", func(t *testing.T) {
		name := "error-with-meta"
		stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))
		if err != nil {
			t.Fatalf("Failed to start stream: %v", err)
		}

		// Drain the stream
		for stream.Receive() {
			// Should not receive any messages
		}

		err = stream.Err()
		if err == nil {
			t.Fatal("Expected error, got none")
		}

		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}

		if connectErr.Code() != connect.CodeInternal {
			t.Fatalf("Expected code 'internal', got %q", connectErr.Code())
		}

		// Check custom metadata is present in trailers
		trailers := stream.ResponseTrailer()
		assertTrailerValue(t, trailers, "x-error-id", "err-12345")
		assertTrailerValue(t, trailers, "x-request-id", "req-67890")
		assertTrailerValue(t, trailers, "x-custom-bin", "AAEC")

		t.Logf("Metadata received: %v", trailers)
	})

	t.Run("protocol headers filtered", func(t *testing.T) {
		name := "error-with-protocol-headers"
		stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))
		if err != nil {
			t.Fatalf("Failed to start stream: %v", err)
		}

		// Drain the stream
		for stream.Receive() {
			// Should not receive any messages
		}

		err = stream.Err()
		if err == nil {
			t.Fatal("Expected error, got none")
		}

		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}
		if connectErr.Code() != connect.CodeInvalidArgument {
			t.Fatalf("Expected code 'invalid_argument', got %q", connectErr.Code())
		}

		trailers := stream.ResponseTrailer()

		// x-custom should be present
		assertTrailerValue(t, trailers, "x-custom", "should-appear")

		// Protocol headers should be filtered out
		assertTrailerAbsent(t, trailers, "content-type")
		assertTrailerAbsent(t, trailers, "grpc-status")
		assertTrailerAbsent(t, trailers, "connect-timeout-ms")

		t.Logf("Metadata received (protocol headers filtered): %v", trailers)
	})

	t.Run("mid-stream error with metadata", func(t *testing.T) {
		name := "mid-stream-error"
		stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))
		if err != nil {
			t.Fatalf("Failed to start stream: %v", err)
		}

		// Count messages before error
		msgCount := 0
		for stream.Receive() {
			msgCount++
		}

		if msgCount != 2 {
			t.Fatalf("Expected 2 messages before error, got %d", msgCount)
		}

		err = stream.Err()
		if err == nil {
			t.Fatal("Expected error, got none")
		}

		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}
		if connectErr.Code() != connect.CodeAborted {
			t.Fatalf("Expected code 'aborted', got %q", connectErr.Code())
		}

		// Check metadata from mid-stream error
		trailers := stream.ResponseTrailer()
		assertTrailerValue(t, trailers, "x-abort-reason", "test-abort")
		assertTrailerValue(t, trailers, "x-message-count", "2")

		t.Logf("Received %d messages, then error with metadata: %v", msgCount, trailers)
	})

	t.Run("successful stream has no metadata", func(t *testing.T) {
		name := "Alice"
		stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))
		if err != nil {
			t.Fatalf("Failed to start stream: %v", err)
		}

		// Count messages
		msgCount := 0
		for stream.Receive() {
			msgCount++
		}

		if err := stream.Err(); err != nil {
			t.Fatalf("Unexpected error: %v", err)
		}

		if msgCount != 3 {
			t.Fatalf("Expected 3 messages, got %d", msgCount)
		}

		// Successful stream should have empty or absent custom metadata
		trailers := stream.ResponseTrailer()
		// Filter out standard headers to check for custom ones
		customMetadata := make(http.Header)
		for key, values := range trailers {
			if !isProtocolHeader(key) {
				customMetadata[key] = values
			}
		}

		if len(customMetadata) > 0 {
			t.Fatalf("Expected empty custom metadata for successful stream, got: %v", customMetadata)
		}

		t.Logf("Received %d messages, EndStream with no custom metadata (as expected)", msgCount)
	})
}

func assertTrailerValue(t *testing.T, trailers http.Header, key, expectedValue string) {
	t.Helper()
	values := trailers.Values(key)
	if len(values) == 0 {
		t.Fatalf("Expected trailer key %q to be present", key)
	}
	found := false
	for _, v := range values {
		if v == expectedValue {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("Expected trailer %q to contain %q, got %v", key, expectedValue, values)
	}
}

func assertTrailerAbsent(t *testing.T, trailers http.Header, key string) {
	t.Helper()
	if values := trailers.Values(key); len(values) > 0 {
		t.Fatalf("Expected trailer key %q to be absent (protocol header should be filtered), got %v", key, values)
	}
}

func isProtocolHeader(key string) bool {
	switch key {
	case "Content-Type", "content-type",
		"Grpc-Status", "grpc-status",
		"Grpc-Message", "grpc-message",
		"Connect-Protocol-Version", "connect-protocol-version":
		return true
	}
	return false
}
