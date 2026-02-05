package main

import (
	"context"
	"fmt"
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
		fmt.Printf("    FAIL  tonic unary via Connect protocol: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic unary via Connect protocol\n")
	}

	// Test 2: gRPC unary
	if err := testGRPCUnary(socketPath); err != nil {
		fmt.Printf("    FAIL  tonic unary via gRPC protocol: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic unary via gRPC protocol\n")
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

func testGRPCUnary(socketPath string) error {
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

	client := gen.NewHelloWorldServiceClient(conn)
	name := "Bob"
	resp, err := client.SayHello(context.Background(), &gen.HelloRequest{Name: &name})
	if err != nil {
		return fmt.Errorf("gRPC unary failed: %w", err)
	}
	if !strings.Contains(resp.Message, "Bob") {
		return fmt.Errorf("expected 'Bob' in response, got: %s", resp.Message)
	}
	return nil
}
