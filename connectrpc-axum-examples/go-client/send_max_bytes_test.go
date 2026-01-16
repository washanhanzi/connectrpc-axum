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

// TestSendMaxBytesUnarySmall tests that small responses succeed when under the send limit.
func TestSendMaxBytesUnarySmall(t *testing.T) {
	s := startServer(t, "send-max-bytes", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// "small" returns a response under 100 bytes - should succeed
	name := "small"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Small response should succeed: %v", err)
	}

	if resp.Msg.Message != "Hi" {
		t.Fatalf("Expected 'Hi', got %q", resp.Msg.Message)
	}

	t.Logf("Small response succeeded: %s", resp.Msg.Message)
}

// TestSendMaxBytesUnaryLarge tests that large responses return ResourceExhausted.
func TestSendMaxBytesUnaryLarge(t *testing.T) {
	s := startServer(t, "send-max-bytes", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// "large" returns a response over 100 bytes - should fail with ResourceExhausted
	name := "large"
	_, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))

	if err == nil {
		t.Fatal("Large response should have failed with ResourceExhausted")
	}

	connectErr := new(connect.Error)
	if !errors.As(err, &connectErr) {
		t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
	}

	if connectErr.Code() != connect.CodeResourceExhausted {
		t.Fatalf("Expected CodeResourceExhausted, got: %v", connectErr.Code())
	}

	t.Logf("Large response correctly returned ResourceExhausted: %s", connectErr.Message())
}

// TestSendMaxBytesStreamSmall tests that streaming with small messages succeeds.
func TestSendMaxBytesStreamSmall(t *testing.T) {
	s := startServer(t, "send-max-bytes", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// "stream_small" returns all messages under 100 bytes - should succeed
	name := "stream_small"
	stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for stream.Receive() {
		msgCount++
		t.Logf("[%d] %s", msgCount, stream.Msg().Message)
	}

	if err := stream.Err(); err != nil {
		t.Fatalf("Stream should succeed with small messages: %v", err)
	}

	if msgCount != 3 {
		t.Fatalf("Expected 3 messages, got %d", msgCount)
	}

	t.Logf("Stream with small messages succeeded: received %d messages", msgCount)
}

// TestSendMaxBytesStreamLarge tests that streaming fails mid-stream when a message exceeds the limit.
func TestSendMaxBytesStreamLarge(t *testing.T) {
	s := startServer(t, "send-max-bytes", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// "stream_large" returns: small (ok), large (fails), small (not sent)
	name := "stream_large"
	stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for stream.Receive() {
		msgCount++
		t.Logf("[%d] %s", msgCount, stream.Msg().Message)
	}

	// Should have received exactly 1 message before the error
	if msgCount != 1 {
		t.Fatalf("Expected 1 message before error, got %d", msgCount)
	}

	// Stream should end with ResourceExhausted error
	err = stream.Err()
	if err == nil {
		t.Fatal("Stream should have failed with ResourceExhausted")
	}

	connectErr := new(connect.Error)
	if !errors.As(err, &connectErr) {
		t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
	}

	if connectErr.Code() != connect.CodeResourceExhausted {
		t.Fatalf("Expected CodeResourceExhausted, got: %v", connectErr.Code())
	}

	t.Logf("Stream correctly failed with ResourceExhausted after %d message(s): %s",
		msgCount, connectErr.Message())
}
