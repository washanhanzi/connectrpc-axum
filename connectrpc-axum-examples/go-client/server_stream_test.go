package main

import (
	"context"
	"io"
	"net/http"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// TestConnectServerStream tests pure Connect protocol server streaming.
func TestConnectServerStream(t *testing.T) {
	s := startServer(t, "connect-server-stream", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Connect Stream Tester"
	stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name:    &name,
		Hobbies: []string{"coding", "testing"},
	}))
	if err != nil {
		t.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for stream.Receive() {
		msgCount++
		msg := stream.Msg().Message
		if msg == "" {
			t.Fatalf("Empty message at position %d", msgCount)
		}
		t.Logf("[%d] %s", msgCount, msg)
	}

	if err := stream.Err(); err != nil {
		t.Fatalf("Stream error: %v", err)
	}

	if msgCount == 0 {
		t.Fatal("Received no messages")
	}

	t.Logf("Received %d messages", msgCount)
}

// TestTonicServerStreamConnect tests tonic server streaming with Connect protocol.
func TestTonicServerStreamConnect(t *testing.T) {
	s := startServer(t, "tonic-server-stream", "tonic")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Tonic Connect Stream Tester"
	stream, err := client.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name:    &name,
		Hobbies: []string{"coding", "testing"},
	}))
	if err != nil {
		t.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for stream.Receive() {
		msgCount++
		msg := stream.Msg().Message
		if msg == "" {
			t.Fatalf("Empty message at position %d", msgCount)
		}
		t.Logf("[%d] %s", msgCount, msg)
	}

	if err := stream.Err(); err != nil {
		t.Fatalf("Stream error: %v", err)
	}

	if msgCount == 0 {
		t.Fatal("Received no messages")
	}

	t.Logf("Received %d messages", msgCount)
}

// TestTonicServerStreamGRPC tests tonic server streaming with gRPC protocol.
func TestTonicServerStreamGRPC(t *testing.T) {
	s := startServer(t, "tonic-server-stream", "tonic")
	defer s.stop()

	conn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewHelloWorldServiceClient(conn)

	name := "gRPC Stream Tester"
	stream, err := client.SayHelloStream(context.Background(), &gen.HelloRequest{
		Name:    &name,
		Hobbies: []string{"coding", "testing"},
	})
	if err != nil {
		t.Fatalf("Failed to start stream: %v", err)
	}

	msgCount := 0
	for {
		resp, err := stream.Recv()
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatalf("Stream error: %v", err)
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

	t.Logf("Received %d messages", msgCount)
}
