package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/test/go/gen"
	"github.com/connectrpc-axum/test/go/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	transport := &http.Transport{
		DialContext: func(_ context.Context, _, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		},
	}
	httpClient := &http.Client{Transport: transport}

	failed := false

	// Test 1: Connect unary
	if err := testConnectUnary(httpClient); err != nil {
		fmt.Printf("    FAIL  tonic bidi server Connect unary: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic bidi server Connect unary\n")
	}

	// Test 2: gRPC bidi stream
	if err := testGRPCBidiStream(socketPath); err != nil {
		fmt.Printf("    FAIL  tonic bidi server gRPC bidi stream: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic bidi server gRPC bidi stream\n")
	}

	// Test 3: gRPC client stream
	if err := testGRPCClientStream(socketPath); err != nil {
		fmt.Printf("    FAIL  tonic bidi server gRPC client stream: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic bidi server gRPC client stream\n")
	}

	if failed {
		os.Exit(1)
	}
}

func testConnectUnary(client *http.Client) error {
	connectClient := genconnect.NewHelloWorldServiceClient(client, "http://localhost")
	name := "Alice"
	resp, err := connectClient.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{Name: &name}))
	if err != nil {
		return fmt.Errorf("Connect unary failed: %w", err)
	}
	if !strings.Contains(resp.Msg.Message, "Alice") {
		return fmt.Errorf("expected 'Alice' in response, got: %s", resp.Msg.Message)
	}
	return nil
}

func testGRPCBidiStream(socketPath string) error {
	conn, err := grpc.NewClient(
		"passthrough:///unix",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		}),
	)
	if err != nil {
		return fmt.Errorf("failed to connect: %w", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)
	stream, err := client.EchoBidiStream(context.Background())
	if err != nil {
		return fmt.Errorf("failed to start bidi stream: %w", err)
	}

	// Send 3 messages
	for _, msg := range []string{"Hello", "World", "Bidi"} {
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			return fmt.Errorf("send error: %w", err)
		}
	}
	if err := stream.CloseSend(); err != nil {
		return fmt.Errorf("close send error: %w", err)
	}

	// Receive responses
	msgCount := 0
	for {
		resp, err := stream.Recv()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("recv error: %w", err)
		}
		msgCount++
		if resp.Message == "" {
			return fmt.Errorf("empty message at position %d", msgCount)
		}
	}

	if msgCount < 2 {
		return fmt.Errorf("expected at least 2 bidi responses, got %d", msgCount)
	}
	return nil
}

func testGRPCClientStream(socketPath string) error {
	conn, err := grpc.NewClient(
		"passthrough:///unix",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		}),
	)
	if err != nil {
		return fmt.Errorf("failed to connect: %w", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)
	stream, err := client.EchoClientStream(context.Background())
	if err != nil {
		return fmt.Errorf("failed to start client stream: %w", err)
	}

	// Send 2 messages
	for _, msg := range []string{"Alice", "Bob"} {
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			return fmt.Errorf("send error: %w", err)
		}
	}

	resp, err := stream.CloseAndRecv()
	if err != nil {
		return fmt.Errorf("close and recv error: %w", err)
	}

	if !strings.Contains(resp.Message, "2 messages") {
		return fmt.Errorf("expected '2 messages' in response, got: %s", resp.Message)
	}
	return nil
}
