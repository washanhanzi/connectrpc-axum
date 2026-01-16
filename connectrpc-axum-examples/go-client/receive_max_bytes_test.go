package main

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestReceiveMaxBytesUnarySmall tests that small requests succeed when under the receive limit.
func TestReceiveMaxBytesUnarySmall(t *testing.T) {
	s := startServer(t, "receive-max-bytes", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// Small message under 1000 bytes - should succeed
	smallMessage := strings.Repeat("a", 100)
	resp, err := client.Echo(context.Background(), connect.NewRequest(&gen.EchoRequest{
		Message: smallMessage,
	}))
	if err != nil {
		t.Fatalf("Small request should succeed: %v", err)
	}

	t.Logf("Small request succeeded: %s", resp.Msg.Message)
}

// TestReceiveMaxBytesUnaryLarge tests that large requests return ResourceExhausted.
func TestReceiveMaxBytesUnaryLarge(t *testing.T) {
	s := startServer(t, "receive-max-bytes", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// Large message over 1000 bytes - should fail with ResourceExhausted
	largeMessage := strings.Repeat("x", 2000)
	_, err := client.Echo(context.Background(), connect.NewRequest(&gen.EchoRequest{
		Message: largeMessage,
	}))

	if err == nil {
		t.Fatal("Large request should have failed with ResourceExhausted")
	}

	connectErr := new(connect.Error)
	if !errors.As(err, &connectErr) {
		t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
	}

	if connectErr.Code() != connect.CodeResourceExhausted {
		t.Fatalf("Expected CodeResourceExhausted, got: %v - %s", connectErr.Code(), connectErr.Message())
	}

	t.Logf("Large request correctly returned ResourceExhausted: %s", connectErr.Message())
}

// TestReceiveMaxBytesStreamSmall tests that streaming with small messages succeeds.
func TestReceiveMaxBytesStreamSmall(t *testing.T) {
	s := startServer(t, "receive-max-bytes", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	stream := client.EchoClientStream(context.Background())

	// Send 5 small messages (each < 1000 bytes)
	for i := 0; i < 5; i++ {
		smallMessage := strings.Repeat("a", 100)
		if err := stream.Send(&gen.EchoRequest{Message: smallMessage}); err != nil {
			t.Fatalf("Failed to send small message %d: %v", i+1, err)
		}
	}

	resp, err := stream.CloseAndReceive()
	if err != nil {
		t.Fatalf("Stream with small messages should succeed: %v", err)
	}

	t.Logf("Stream with small messages succeeded: %s", resp.Msg.Message)
}

// TestReceiveMaxBytesStreamLarge tests that streaming fails when a message exceeds the limit.
func TestReceiveMaxBytesStreamLarge(t *testing.T) {
	s := startServer(t, "receive-max-bytes", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	stream := client.EchoClientStream(context.Background())

	// First send a small message (should succeed)
	smallMessage := strings.Repeat("a", 100)
	if err := stream.Send(&gen.EchoRequest{Message: smallMessage}); err != nil {
		t.Fatalf("Failed to send small message: %v", err)
	}

	// Then send a large message (should trigger error)
	largeMessage := strings.Repeat("x", 2000)
	if err := stream.Send(&gen.EchoRequest{Message: largeMessage}); err != nil {
		// Error might happen on send
		t.Logf("Error on send (expected): %v", err)
	}

	// Close and receive - should get ResourceExhausted error
	_, err := stream.CloseAndReceive()
	if err == nil {
		t.Fatal("Stream with large message should have failed with ResourceExhausted")
	}

	connectErr := new(connect.Error)
	if !errors.As(err, &connectErr) {
		t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
	}

	if connectErr.Code() != connect.CodeResourceExhausted {
		t.Fatalf("Expected CodeResourceExhausted, got: %v - %s", connectErr.Code(), connectErr.Message())
	}

	t.Logf("Stream with large message correctly returned ResourceExhausted: %s", connectErr.Message())
}

// =============================================================================
// Tests for 5MB limit
// =============================================================================

// TestReceiveMaxBytes5MBUnarySmall tests that requests under 5MB succeed.
func TestReceiveMaxBytes5MBUnarySmall(t *testing.T) {
	s := startServer(t, "receive-max-bytes-5mb", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// 4MB message - should succeed (under 5MB limit)
	message := strings.Repeat("a", 4*1024*1024)
	resp, err := client.Echo(context.Background(), connect.NewRequest(&gen.EchoRequest{
		Message: message,
	}))
	if err != nil {
		t.Fatalf("4MB request should succeed with 5MB limit: %v", err)
	}

	t.Logf("4MB request succeeded: %s", resp.Msg.Message)
}

// TestReceiveMaxBytes5MBUnaryLarge tests that requests over 5MB fail.
func TestReceiveMaxBytes5MBUnaryLarge(t *testing.T) {
	s := startServer(t, "receive-max-bytes-5mb", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// 6MB message - should fail (over 5MB limit)
	message := strings.Repeat("x", 6*1024*1024)
	_, err := client.Echo(context.Background(), connect.NewRequest(&gen.EchoRequest{
		Message: message,
	}))

	if err == nil {
		t.Fatal("6MB request should have failed with 5MB limit")
	}

	connectErr := new(connect.Error)
	if !errors.As(err, &connectErr) {
		t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
	}

	if connectErr.Code() != connect.CodeResourceExhausted {
		t.Fatalf("Expected CodeResourceExhausted, got: %v", connectErr.Code())
	}

	t.Logf("6MB request correctly returned ResourceExhausted: %s", connectErr.Message())
}

// TestReceiveMaxBytes5MBStreamSmall tests streaming under 5MB limit.
func TestReceiveMaxBytes5MBStreamSmall(t *testing.T) {
	s := startServer(t, "receive-max-bytes-5mb", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	stream := client.EchoClientStream(context.Background())

	// Send 4 messages of 1MB each (4MB total, under 5MB per-message limit)
	for i := 0; i < 4; i++ {
		message := strings.Repeat("a", 1024*1024)
		if err := stream.Send(&gen.EchoRequest{Message: message}); err != nil {
			t.Fatalf("Failed to send 1MB message %d: %v", i+1, err)
		}
	}

	resp, err := stream.CloseAndReceive()
	if err != nil {
		t.Fatalf("Stream should succeed: %v", err)
	}

	t.Logf("Stream succeeded: %s", resp.Msg.Message)
}

// =============================================================================
// Tests for unlimited
// =============================================================================

// TestReceiveMaxBytesUnlimitedUnary tests that large requests succeed with unlimited.
func TestReceiveMaxBytesUnlimitedUnary(t *testing.T) {
	s := startServer(t, "receive-max-bytes-unlimited", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	// 6MB message - should succeed with unlimited
	message := strings.Repeat("a", 6*1024*1024)
	resp, err := client.Echo(context.Background(), connect.NewRequest(&gen.EchoRequest{
		Message: message,
	}))
	if err != nil {
		t.Fatalf("6MB request should succeed with unlimited: %v", err)
	}

	t.Logf("6MB request succeeded: %s", resp.Msg.Message)
}

// TestReceiveMaxBytesUnlimitedStream tests that large stream messages succeed with unlimited.
func TestReceiveMaxBytesUnlimitedStream(t *testing.T) {
	s := startServer(t, "receive-max-bytes-unlimited", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	stream := client.EchoClientStream(context.Background())

	// Send 3 messages of 3MB each (9MB total)
	for i := 0; i < 3; i++ {
		message := strings.Repeat("a", 3*1024*1024)
		if err := stream.Send(&gen.EchoRequest{Message: message}); err != nil {
			t.Fatalf("Failed to send 3MB message %d: %v", i+1, err)
		}
	}

	resp, err := stream.CloseAndReceive()
	if err != nil {
		t.Fatalf("Stream should succeed with unlimited: %v", err)
	}

	t.Logf("Stream with large messages succeeded: %s", resp.Msg.Message)
}
