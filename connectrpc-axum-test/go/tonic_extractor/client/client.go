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
	"google.golang.org/grpc/metadata"
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

	// Test 1: Connect without key
	if err := testConnectWithoutKey(httpClient); err != nil {
		fmt.Printf("    FAIL  Connect without api key: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  Connect without api key\n")
	}

	// Test 2: Connect with key
	if err := testConnectWithKey(httpClient); err != nil {
		fmt.Printf("    FAIL  Connect with api key: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  Connect with api key\n")
	}

	// Test 3: gRPC without key
	if err := testGRPCWithoutKey(socketPath); err != nil {
		fmt.Printf("    FAIL  gRPC without api key: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  gRPC without api key\n")
	}

	// Test 4: gRPC with key
	if err := testGRPCWithKey(socketPath); err != nil {
		fmt.Printf("    FAIL  gRPC with api key: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  gRPC with api key\n")
	}

	if failed {
		os.Exit(1)
	}
}

func testConnectWithoutKey(httpClient *http.Client) error {
	connectClient := genconnect.NewHelloWorldServiceClient(httpClient, "http://localhost")
	name := "Alice"
	_, err := connectClient.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{Name: &name}))
	if err == nil {
		return fmt.Errorf("expected unauthenticated error, got success")
	}
	if connect.CodeOf(err) != connect.CodeUnauthenticated {
		return fmt.Errorf("expected unauthenticated code, got: %v", connect.CodeOf(err))
	}
	return nil
}

func testConnectWithKey(httpClient *http.Client) error {
	connectClient := genconnect.NewHelloWorldServiceClient(httpClient, "http://localhost")
	name := "Alice"
	req := connect.NewRequest(&gen.HelloRequest{Name: &name})
	req.Header().Set("x-api-key", "test-key-123")
	resp, err := connectClient.SayHello(context.Background(), req)
	if err != nil {
		return fmt.Errorf("Connect with key failed: %w", err)
	}
	if !strings.Contains(resp.Msg.Message, "Alice") {
		return fmt.Errorf("expected 'Alice' in response, got: %s", resp.Msg.Message)
	}
	if !strings.Contains(resp.Msg.Message, "key:") {
		return fmt.Errorf("expected 'key:' in response, got: %s", resp.Msg.Message)
	}
	return nil
}

func testGRPCWithoutKey(socketPath string) error {
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
	_, err = client.SayHello(context.Background(), &gen.HelloRequest{Name: &name})
	if err == nil {
		return fmt.Errorf("expected unauthenticated error, got success")
	}
	return nil
}

func testGRPCWithKey(socketPath string) error {
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
	md := metadata.Pairs("x-api-key", "test-key-123")
	ctx := metadata.NewOutgoingContext(context.Background(), md)
	resp, err := client.SayHello(ctx, &gen.HelloRequest{Name: &name})
	if err != nil {
		return fmt.Errorf("gRPC with key failed: %w", err)
	}
	if !strings.Contains(resp.Message, "Bob") {
		return fmt.Errorf("expected 'Bob' in response, got: %s", resp.Message)
	}
	if !strings.Contains(resp.Message, "key:") {
		return fmt.Errorf("expected 'key:' in response, got: %s", resp.Message)
	}
	return nil
}
