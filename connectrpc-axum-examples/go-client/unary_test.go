package main

import (
	"context"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// TestConnectUnary tests pure Connect protocol unary RPC.
func TestConnectUnary(t *testing.T) {
	s := startServer(t, "connect-unary", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Connect Unary Tester"
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

// TestTonicUnaryConnect tests tonic server with Connect protocol.
func TestTonicUnaryConnect(t *testing.T) {
	s := startServer(t, "tonic-unary", "tonic")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Tonic Connect Unary Tester"
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

// TestTonicUnaryGRPC tests tonic server with gRPC protocol.
func TestTonicUnaryGRPC(t *testing.T) {
	s := startServer(t, "tonic-unary", "tonic")
	defer s.stop()

	conn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewHelloWorldServiceClient(conn)

	name := "gRPC Unary Tester"
	resp, err := client.SayHello(context.Background(), &gen.HelloRequest{
		Name: &name,
	})
	if err != nil {
		t.Fatalf("gRPC unary failed: %v", err)
	}

	if resp.Message == "" {
		t.Fatal("Empty response message")
	}
	if !strings.Contains(resp.Message, name) {
		t.Fatalf("Response doesn't contain name: got %q", resp.Message)
	}

	t.Logf("Response: %s", resp.Message)
}
