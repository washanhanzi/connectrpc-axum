package main

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// TestTonicBidiStreamConnectUnary tests Connect unary RPC on a server that also supports bidi streaming.
func TestTonicBidiStreamConnectUnary(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Bidi Server Unary Tester"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Connect unary failed: %v", err)
	}

	if resp.Msg.Message == "" {
		t.Fatal("Empty response message")
	}
	if !strings.Contains(resp.Msg.Message, name) {
		t.Fatalf("Response doesn't contain name: got %q", resp.Msg.Message)
	}

	t.Logf("Response: %s", resp.Msg.Message)
}

// TestTonicBidiStreamGRPC tests gRPC bidirectional streaming.
func TestTonicBidiStreamGRPC(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	conn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)

	stream, err := client.EchoBidiStream(context.Background())
	if err != nil {
		t.Fatalf("Failed to start bidi stream: %v", err)
	}

	// Send messages in a goroutine
	messages := []string{"Hello", "World", "Bidi", "Stream", "Test"}
	go func() {
		for _, msg := range messages {
			if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
				t.Logf("Send error: %v", err)
				return
			}
			time.Sleep(50 * time.Millisecond)
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
			t.Fatalf("Recv error: %v", err)
		}
		msgCount++
		if resp.Message == "" {
			t.Fatalf("Empty message at position %d", msgCount)
		}
		t.Logf("[%d] %s", msgCount, resp.Message)
	}

	if msgCount == 0 {
		t.Fatal("Received no messages")
	}

	t.Logf("Bidi stream completed with %d messages", msgCount)
}
